//! Агент-рантайм: режим `aga agent` — отдельный процесс того же бинаря.
//!
//! Чат (HTTP-сервер `aga`) про агентов ничего не знает: он пишет сообщения
//! в БД и публикует события в Centrifugo. Этот процесс подписан на общий канал
//! и обслуживает пользователей, к которым привязаны агенты
//! (`agents.listen_user_id`): агент отвечает от имени своего пользователя X.
//!
//! Триггеры (чат-сессии проекта, чей набор содержит агента):
//! - чужое сообщение упоминает `@X` — X спрашивают;
//! - любое чужое сообщение в нити, которую начал X;
//! - ответ (`parent_id`) на вопрос агента (`ask_human`) — от любого участника.
//!
//! Свои сообщения X и шаги работы (`kind='step'`) не триггерят. Одно сообщение
//! может запустить нескольких агентов. Переписки ограничены глубиной нитей
//! (`AGA_MAX_THREAD_DEPTH`) и числом сообщений в нити
//! (`AGA_MAX_THREAD_MESSAGES`), см. `chat::max_chat_level`/`max_thread_messages`.
//!
//! Ход работы публикуется в чат: шаг — команда в теле, вывод в скрытой части,
//! вызов JSON-ом (`messages.tool_call`), по нему контекст (`context.rs`)
//! восстанавливает прошлые вызовы агента.
//!
//! Параллелизм: события диспатчатся асинхронно (spawn), запуск агента
//! ограничивается семафором общего параллелизма; один и тот же агент на одной
//! станции — строго последовательно (FIFO-мьютекс на пару «станция+агент»),
//! разные агенты станции работают параллельно — и LLM-шаги, и команды: каждая
//! команда идёт в песочнице владения агента (пишет только в свою зону), так
//! что агенты не мешают друг другу и сразу видят чужие изменения.
//!
//! Centrifugo — обязательная зависимость агентов: нет подписки — нет реакций
//! (чат при этом работает). Внутренней fallback-шины нет намеренно: один
//! источник событий проще.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::Semaphore;

use crate::agent::{Agent, AgentOutcome, Step, ThreadStarter};
use crate::centrifuge::{parse_sse_event, CentrifugeClient};
use crate::chat::{mentioned_names, ChatStore, KIND_STEP};
use crate::cluster::Cluster;
use crate::config::{Config, RoleConfig};
use crate::llm::LlmClient;
use crate::scope::Territory;
use crate::trace::{AgentSet, TraceStore};
use crate::workstation::executor_for_workstation;

/// Пауза перед переподключением к Centrifugo после обрыва (сек).
const RECONNECT_SECS: u64 = 2;
/// Пауза, когда слушать нечего (ни одного привязанного агента) — перепроверка
/// настроек дешевле, чем соединение с пустым списком каналов (сек).
const IDLE_WAIT_SECS: u64 = 10;
/// Максимум одновременно работающих агентов (LLM+exec) в процессе.
const MAX_CONCURRENT_AGENT_RUNS: usize = 4;
/// Как часто живая подписка прерывается на переподключение, чтобы перечитать
/// каналы (новая сессия/привязка подхватывается без рестарта), сек.
const CHANNELS_REFRESH_SECS: u64 = 30;

/// Собрать конфиг агента из набора: промпт = правила + данные агенту скиллы
/// (единственное содержимое каталога), инструменты — отдельный список
/// без версий; MCP-серверы — из каталога по именам; территория — папка узла
/// в дереве набора; LLM — выбранное
/// подключение (url и ключ), без подключения — дефолтная LLM.
pub async fn resolve_agent(
    store: &TraceStore,
    set: &AgentSet,
    name: &str,
) -> Result<Option<(RoleConfig, Territory)>, sqlx::Error> {
    let Some(agent) = set.agents.iter().find(|a| a.name == name) else {
        return Ok(None);
    };
    let prompt = store.agent_prompt(agent).await?;
    let config = RoleConfig {
        prompt,
        tools: agent.tools.clone(),
        max_iterations: agent.max_iterations,
        llm: store.llm_config_for(agent).await?,
        mcp_servers: store.mcp_servers_by_names(&agent.mcp).await?,
    };
    Ok(Some((config, crate::scope::territory_for(set, agent))))
}

/// Сериализующий мьютекс запуска/станции, доступный из нескольких задач.
type RunLock = Arc<AsyncMutex<()>>;

/// Общие структуры диспетчера: локи сериализации и множество уже принятых
/// сообщений (защита от redelivery Centrifugo).
#[derive(Default)]
struct RuntimeState {
    /// FIFO на пару «станция + агент»: один агент никогда не работает в двух
    /// экземплярах на одной станции (ws=None — локальный запуск, тоже очередь).
    agent_locks: StdMutex<HashMap<(Option<i64>, String), RunLock>>,
    /// message_id уже принятых событий — повторная доставка не запускает агента.
    seen: StdMutex<HashSet<i64>>,
}

impl RuntimeState {
    fn agent_lock(&self, key: (Option<i64>, String)) -> RunLock {
        let mut map = self.agent_locks.lock().unwrap();
        Arc::clone(
            map.entry(key)
                .or_insert_with(|| Arc::new(AsyncMutex::new(()))),
        )
    }

    /// true — сообщение принято впервые; false — redelivery, пропустить.
    fn mark_seen(&self, message_id: i64) -> bool {
        self.seen.lock().unwrap().insert(message_id)
    }
}

/// Задача агента, размеченная роутером: кто отвечает, откуда контекст, где исполнять.
struct Job {
    chat_id: i64,
    /// Пользователь, которого обслуживает агент: ответ — от его имени.
    author_id: i64,
    agent_name: String,
    set: AgentSet,
    ws_id: Option<i64>,
    /// Сообщение, на которое отвечает агент (последнее в контексте).
    trigger_id: i64,
}

#[derive(Clone)]
pub struct AgentRuntime {
    llm_client: LlmClient,
    trace_store: TraceStore,
    chat_store: ChatStore,
    cluster: Cluster,
    centrifuge: CentrifugeClient,
    state: Arc<RuntimeState>,
    permits: Arc<Semaphore>,
}

impl AgentRuntime {
    pub fn new(
        llm_client: LlmClient,
        trace_store: TraceStore,
        chat_store: ChatStore,
        cluster: Cluster,
        centrifuge: CentrifugeClient,
    ) -> Self {
        Self {
            llm_client,
            trace_store,
            chat_store,
            cluster,
            centrifuge,
            state: Arc::new(RuntimeState::default()),
            permits: Arc::new(Semaphore::new(MAX_CONCURRENT_AGENT_RUNS)),
        }
    }

    /// Каналы для подписки: общий канал (туда публикуется каждое сообщение
    /// любого чата и нити — упоминание `@X` может прийти откуда угодно). Нет ни
    /// одного привязанного агента — слушать нечего.
    pub async fn listen_channels(&self) -> Result<Vec<String>, sqlx::Error> {
        if self.trace_store.listen_user_ids().await?.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![self.centrifuge.channel()])
    }

    /// Главный цикл процесса: подписка на Centrifugo, разбор событий,
    /// переподключение при обрыве. Привязки перечитываются при каждом
    /// переподключении — новая привязка подхватывается без рестарта.
    pub async fn run(&self) {
        loop {
            let channels = self.listen_channels().await.unwrap_or_default();
            if channels.is_empty() {
                tokio::time::sleep(std::time::Duration::from_secs(IDLE_WAIT_SECS)).await;
                continue;
            }
            match self.session(&channels).await {
                Ok(()) => tracing::info!("agent runtime: подписка завершена, переподключение"),
                Err(e) => tracing::warn!("agent runtime: ошибка подписки: {e}"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(RECONNECT_SECS)).await;
        }
    }

    /// Одна подписка: SSE-стрим Centrifugo с серверными подписками из токена.
    /// События только диспатчатся — запуски агентов идут в отдельных задачах,
    /// медленный агент не блокирует чтение стрима и другие каналы.
    /// Раз в `CHANNELS_REFRESH_SECS` стрим прерывается на переподключение, чтобы
    /// перечитать каналы: новые/закрытые сессии и привязки подхватываются без
    /// рестарта процесса (на живом соединении серверные подписки не меняются).
    async fn session(
        &self,
        channels: &[String],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let token = self.centrifuge.subscriber_jwt(channels)?;
        let response = self.centrifuge.sse_stream(&token).await?;
        let mut stream = response.bytes_stream();
        let mut buffer: Vec<u8> = Vec::new();
        let mut refresh = tokio::time::interval_at(
            tokio::time::Instant::now() + std::time::Duration::from_secs(CHANNELS_REFRESH_SECS),
            std::time::Duration::from_secs(CHANNELS_REFRESH_SECS),
        );
        refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = refresh.tick() => return Ok(()), // плановое переподключение
                chunk = stream.next() => {
                    let Some(chunk) = chunk else { return Ok(()) };
                    buffer.extend_from_slice(&chunk?);
                    // Кадры SSE разделены пустой строкой; комментарий-пин — не событие.
                    while let Some(pos) = find_frame_end(&buffer) {
                        let frame = String::from_utf8_lossy(&buffer[..pos]).to_string();
                        buffer.drain(..pos + 2);
                        if let Some((_channel, data)) = parse_sse_event(&frame) {
                            self.on_event(&data).await;
                        }
                    }
                }
            }
        }
    }

    async fn on_event(&self, data: &Value) {
        if data["type"] != "message" {
            return; // lifecycle-события — дело веба, агентов они не касаются
        }
        let Some(message_id) = data["message_id"].as_i64() else {
            return;
        };
        if !self.state.mark_seen(message_id) {
            return; // redelivery Centrifugo — запуск уже идёт или прошёл
        }
        let runtime = self.clone();
        tokio::spawn(async move {
            runtime.dispatch(message_id).await;
        });
    }

    /// Путь события из Centrifugo: лимит общего параллелизма, далее — handle_message.
    async fn dispatch(&self, message_id: i64) {
        let Ok(_permit) = self.permits.acquire().await else {
            return;
        };
        self.handle_message(message_id).await;
    }

    /// Обработка события сообщения: найти агентов, которых оно вызывает, и
    /// запустить их параллельно; один агент на станции сериализуется
    /// мьютексом пары «станция+агент». Возвращает true, если хоть один агент
    /// отработал (ответ записан).
    pub async fn handle_message(&self, message_id: i64) -> bool {
        let jobs = self.route(message_id).await;
        let runs = jobs.iter().map(|job| async move {
            let lock = self.state.agent_lock((job.ws_id, job.agent_name.clone()));
            let _serial = lock.lock().await;
            self.run_job(job).await
        });
        futures_util::future::join_all(runs)
            .await
            .into_iter()
            .any(|ok| ok)
    }

    /// Разметка сообщения: каких агентов оно вызывает (см. триггеры в шапке
    /// модуля), на какой станции исполнять. Пусто — сообщение не для агентов.
    async fn route(&self, message_id: i64) -> Vec<Job> {
        let msg = match self.chat_store.get_message(message_id).await {
            Ok(Some(msg)) => msg,
            Ok(None) => return Vec::new(),
            Err(e) => {
                tracing::warn!("runtime: не удалось прочитать сообщение {message_id}: {e}");
                return Vec::new();
            }
        };
        if msg.kind == KIND_STEP {
            return Vec::new(); // шаги работы — лог, а не реплики
        }
        let Ok(Some(project_id)) = self.chat_store.project_id_for_chat(msg.chat_id).await else {
            return Vec::new(); // чат не сессия проекта — набору агентов неоткуда взяться
        };
        let Ok(Some(set)) = self.trace_store.get_project_agent_set(project_id).await else {
            return Vec::new();
        };
        let ws_id = self
            .chat_store
            .root_workstation_id(msg.chat_id)
            .await
            .unwrap_or(None);
        let job = |agent: &crate::trace::AgentDef, user_id: i64| Job {
            chat_id: msg.chat_id,
            author_id: user_id,
            agent_name: agent.name.clone(),
            set: set.clone(),
            ws_id,
            trigger_id: msg.id,
        };

        let mut jobs: Vec<Job> = Vec::new();
        if let Some(parent) = msg.parent_id {
            if let Some(answer) = self.route_answer(&set, &msg, parent, ws_id).await {
                jobs.push(answer);
            }
        }
        // Кого спрашивают: упомянутые пользователи с привязанным агентом.
        let mut asked: Vec<i64> = Vec::new();
        let text = format!("{} {}", msg.title.clone().unwrap_or_default(), msg.body);
        for name in mentioned_names(&text) {
            if let Ok(Some(user)) = self.chat_store.find_user_by_name(&name).await {
                asked.push(user.id);
            }
        }
        // Нить, начатая пользователем X, — X реагирует на каждое сообщение в ней.
        if let Ok(Some(chat)) = self.chat_store.get_chat(msg.chat_id).await {
            if chat.start_message_id.is_some() {
                asked.push(chat.created_by_id);
            }
        }
        for user_id in asked {
            if user_id == msg.author_id || jobs.iter().any(|j| j.author_id == user_id) {
                continue; // своё сообщение не вызывает своего агента; один запуск на агента
            }
            if let Some(agent) = self.trace_store.agent_listening_to(&set, user_id) {
                jobs.push(job(&agent, user_id));
            }
        }
        jobs
    }

    /// Сообщение — ответ на вопрос агента (parent указывает на сообщение-вопрос
    /// pending-запроса этого же чата)? Запрос закрывается, продолжается агент,
    /// задавший вопрос; отвечать может любой участник.
    async fn route_answer(
        &self,
        set: &AgentSet,
        msg: &crate::chat::Message,
        parent: i64,
        ws_id: Option<i64>,
    ) -> Option<Job> {
        let Ok(Some(req)) = self.trace_store.pending_for_question_message(parent).await else {
            return None;
        };
        if req.chat_id != msg.chat_id {
            return None;
        }
        let Ok(true) = self
            .trace_store
            .answer_human_request(&req.id, &msg.body)
            .await
        else {
            return None; // гонка: запрос уже закрыт другим ответом
        };
        let _ = self
            .trace_store
            .complete_task(&req.task_id, "answered")
            .await;
        let agent = set.agents.iter().find(|a| a.name == req.agent_name)?;
        Some(Job {
            chat_id: msg.chat_id,
            // Ответ агента — от его имени (связанного пользователя), реплику
            // мог написать любой участник.
            author_id: agent.listen_user_id.unwrap_or(msg.author_id),
            agent_name: agent.name.clone(),
            set: set.clone(),
            ws_id,
            trigger_id: msg.id,
        })
    }

    /// Запуск цикла агента по размеченной задаче и ответ в чат от имени автора.
    async fn run_job(&self, job: &Job) -> bool {
        let Some((role_config, territory)) =
            resolve_agent(&self.trace_store, &job.set, &job.agent_name)
                .await
                .unwrap_or(None)
        else {
            tracing::error!("runtime: агент {} не найден в наборе", job.agent_name);
            return false;
        };

        let native = role_config.llm.native_tools;
        let context = match crate::context::build_context(
            &self.chat_store,
            job.chat_id,
            job.author_id,
            job.trigger_id,
            native,
        )
        .await
        {
            Ok(context) => context,
            Err(e) => {
                tracing::error!("runtime: не собрать контекст чата {}: {e}", job.chat_id);
                return false;
            }
        };

        let executor = executor_for_workstation(job.ws_id, &self.cluster);
        // Территория действует в воркстейшне (есть под/контейнер с проектом);
        // локальный запуск без воркстейшна границы не имеет.
        let scope = job.ws_id.map(|_| territory);
        let runner = Agent::with_executor(
            role_config,
            self.llm_client.clone(),
            self.trace_store.clone(),
            executor,
            scope,
        )
        .with_thread_starter(self.thread_starter(job));

        let _ = self
            .chat_store
            .ensure_participant(job.chat_id, job.author_id)
            .await;

        // Ход работы: каждый шаг публикуем в чат — вызов инструмента шагом
        // (`kind='step'`: команда в теле, вывод в скрытой части — обрезанный,
        // полный в трассе; вызов JSON-ом), пояснение модели — обычным
        // сообщением. Шаги никого не вызывают. Постер живёт, пока работает
        // прогон: канал закроется вместе с отправителем по выходе из run().
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Step>();
        let chat_store = self.chat_store.clone();
        let centrifuge = self.centrifuge.clone();
        let (chat_id, author_id) = (job.chat_id, job.author_id);
        let poster = tokio::spawn(async move {
            while let Some(step) = rx.recv().await {
                let sent = match &step.tool_call {
                    Some(call) => {
                        chat_store
                            .send_step(
                                chat_id,
                                author_id,
                                &step.body,
                                &truncate_output(&step.hidden),
                                &call.to_string(),
                                "agent",
                            )
                            .await
                    }
                    None => {
                        chat_store
                            .send_message_with_origin(
                                chat_id, author_id, &step.body, "", None, None, "agent",
                            )
                            .await
                    }
                };
                if let Ok(Some(step)) = sent {
                    centrifuge
                        .publish_message(chat_id, step.id, author_id)
                        .await;
                }
            }
        });

        let task_id = uuid::Uuid::new_v4().to_string();
        let text = match runner.run(&task_id, context, Some(tx)).await {
            Ok(AgentOutcome::Question {
                request_id,
                question,
            }) => {
                let _ = poster.await;
                // В чат — сам текст вопроса; ответ — «ответ на это сообщение».
                let Ok(Some(q)) = self
                    .chat_store
                    .send_message_with_origin(
                        job.chat_id,
                        job.author_id,
                        &question,
                        "",
                        None,
                        None,
                        "agent",
                    )
                    .await
                else {
                    return false;
                };
                let _ = self
                    .trace_store
                    .link_human_request_chat(&request_id, job.chat_id, q.id, &job.agent_name)
                    .await;
                self.centrifuge
                    .publish_message(job.chat_id, q.id, job.author_id)
                    .await;
                return true;
            }
            Ok(AgentOutcome::Answer(text)) => text,
            Err(e) => {
                tracing::error!("runtime: агент {name} упал: {e}", name = job.agent_name);
                format!("Ошибка: {e}")
            }
        };
        // Шаги уже в чате — финальный ответ идёт после них.
        let _ = poster.await;

        let Ok(Some(reply)) = self
            .chat_store
            .send_message_with_origin(job.chat_id, job.author_id, &text, "", None, None, "agent")
            .await
        else {
            return false;
        };
        let _ = self
            .chat_store
            .add_artifact(reply.id, "result", Some("Ответ агента"), &text)
            .await;
        self.centrifuge
            .publish_message(job.chat_id, reply.id, job.author_id)
            .await;
        true
    }
}

impl AgentRuntime {
    /// Инструмент `start_thread` агента: нить от сообщения, на которое он
    /// отвечает, первым сообщением — от имени его пользователя. Ответы в нити
    /// вызовут агента снова (нить начата его пользователем).
    fn thread_starter(&self, job: &Job) -> ThreadStarter {
        let chat_store = self.chat_store.clone();
        let centrifuge = self.centrifuge.clone();
        let (chat_id, trigger_id, author_id) = (job.chat_id, job.trigger_id, job.author_id);
        Arc::new(move |title: String, message: String| {
            let chat_store = chat_store.clone();
            let centrifuge = centrifuge.clone();
            Box::pin(async move {
                match chat_store
                    .start_thread(chat_id, trigger_id, &title, &message, "", author_id)
                    .await
                {
                    Ok(Some((thread, first))) => {
                        centrifuge
                            .publish_message(chat_id, first.id, author_id)
                            .await;
                        centrifuge
                            .publish_message(thread.id, first.id, author_id)
                            .await;
                        Ok(format!(
                            "Нить «{title}» начата. Ответы в ней придут тебе отдельно — \
                             сейчас закончи текущий ответ."
                        ))
                    }
                    Ok(None) => Err(format!(
                        "нить не начата: чат закрыт или достигнута глубина нитей ({})",
                        crate::chat::max_chat_level()
                    )),
                    Err(e) => Err(e.to_string()),
                }
            })
        })
    }
}

/// Скрытая часть сообщения-шага: длинный вывод команды обрезается с
/// сохранением начала и конца — человеку достаточно обзора, полная трасса в БД.
fn truncate_output(output: &str) -> String {
    const MAX_CHARS: usize = 3000;
    const HEAD: usize = 2000;
    const TAIL: usize = 1000;
    let count = output.chars().count();
    if count <= MAX_CHARS {
        return output.to_string();
    }
    let head: String = output.chars().take(HEAD).collect();
    let tail: String = output.chars().skip(count - TAIL).collect();
    format!("{head}\n…(вывод обрезан, полное — в трассе задачи)…\n{tail}")
}

/// Точка входа режима `aga agent`: конфиг, БД, подписка. SSO/JWKS не нужны —
/// процесс серверный, HTTP не раздаёт.
pub async fn agent_main(db_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_path =
        std::env::var("AGA_CONFIG_PATH").unwrap_or_else(|_| "./config/roles.yaml".to_string());
    let config = Config::load(&config_path)?;
    // Centrifugo — обязательная зависимость агент-процесса: единственный
    // источник событий. Без него запуск бессмысленен.
    let Some(centrifuge_cfg) = config.centrifuge.as_ref() else {
        return Err(
            "aga agent требует настроенного Centrifugo (блок `centrifuge:` в конфиге)".into(),
        );
    };
    let centrifuge = CentrifugeClient::from_config(centrifuge_cfg);
    let trace_store = TraceStore::new(db_path).await?;
    let chat_store = ChatStore::new(db_path).await?;
    let cluster = Cluster::from_env();
    let runtime = AgentRuntime::new(
        LlmClient::new(),
        trace_store,
        chat_store,
        cluster,
        centrifuge,
    );
    tracing::info!("aga agent: рантайм запущен, HTTP-сервер не поднят");
    runtime.run().await;
    Ok(())
}

/// Позиция разделителя кадров SSE (пустая строка `\n\n`), либо None.
fn find_frame_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(2).position(|w| w == b"\n\n")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::trace::{AgentCapability, AgentSpec, CapabilityKind};

    async fn temp_stores() -> (TraceStore, ChatStore, std::path::PathBuf) {
        let file =
            std::env::temp_dir().join(format!("aga_runtime_test_{}.db", uuid::Uuid::new_v4()));
        let path = file.to_string_lossy().into_owned();
        let trace = TraceStore::new(&path).await.unwrap();
        let chat = ChatStore::new(&path).await.unwrap();
        (trace, chat, file)
    }

    async fn cleanup(file: &std::path::PathBuf) {
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    }

    fn cluster() -> Cluster {
        Cluster {
            backend: crate::cluster::Backend::K8s,
            kubectl: "kubectl".into(),
            namespace: "default".into(),
            template: "/nonexistent.yaml".into(),
            image: "img".into(),
            wait_timeout_secs: 1,
        }
    }

    /// Мок LLM с последовательностью ответов: каждый запрос получает следующий
    /// (после исчерпания — последний).
    async fn mock_llm_seq(answers: Vec<&'static str>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let idx = Arc::new(AtomicUsize::new(0));
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move || {
                let answers = answers.clone();
                let idx = Arc::clone(&idx);
                async move {
                    let i = idx.fetch_add(1, Ordering::SeqCst).min(answers.len() - 1);
                    // JSON-объект — сообщение ассистента целиком (с tool_calls),
                    // иначе — ответ текстом.
                    let message = serde_json::from_str::<serde_json::Value>(answers[i])
                        .ok()
                        .filter(|v| v.is_object())
                        .unwrap_or_else(
                            || serde_json::json!({"role": "assistant", "content": answers[i]}),
                        );
                    axum::Json(serde_json::json!({ "choices": [{ "message": message }] }))
                }
            }),
        );
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/v1"), handle)
    }

    /// Нативный вызов `ask_human` (режим LLM-подключения по умолчанию).
    const ASK_PORT: &str = r#"{"role":"assistant","content":null,"tool_calls":[{"id":"q1","type":"function","function":{"name":"ask_human","arguments":"{\"question\":\"Какой порт открыть?\"}"}}]}"#;
    const ASK_PORT_SHORT: &str = r#"{"role":"assistant","content":null,"tool_calls":[{"id":"q1","type":"function","function":{"name":"ask_human","arguments":"{\"question\":\"Порт?\"}"}}]}"#;

    fn spec(name: &str, listen_user_id: Option<i64>) -> AgentSpec {
        AgentSpec {
            name: name.to_string(),
            description: format!("Правила {name}"),
            tools: vec!["git".to_string()],
            max_iterations: 3,
            llm_id: None,
            parent: None,
            skills: vec![],
            listen_user_id,
            mcp: vec![],
        }
    }

    /// Фикстура: проект с набором (агент bound слушает user), готовая станция,
    /// сессия-чат проекта. Возвращает (рантайм, id слушателя, id чата).
    async fn fixture(
        answer: &'static str,
    ) -> (
        AgentRuntime,
        i64,
        i64,
        TraceStore,
        ChatStore,
        std::path::PathBuf,
        tokio::task::JoinHandle<()>,
    ) {
        fixture_seq(vec![answer]).await
    }

    /// То же, но LLM отвечает по последовательности (многошаговые прогоны).
    async fn fixture_seq(
        answers: Vec<&'static str>,
    ) -> (
        AgentRuntime,
        i64,
        i64,
        TraceStore,
        ChatStore,
        std::path::PathBuf,
        tokio::task::JoinHandle<()>,
    ) {
        let (trace, chat, file) = temp_stores().await;
        let (api_url, llm_server) = mock_llm_seq(answers).await;
        let conn = trace
            .create_llm_connection(&crate::trace::LlmConnectionSpec {
                name: "mock".into(),
                api_url,
                api_key: None,
                model_name: "m".into(),
                native_tools: true,
            })
            .await
            .unwrap();
        trace.set_default_llm(conn).await.unwrap();
        let listener_user = chat
            .insert_user("alice", "human", false, None, None)
            .await
            .unwrap();
        let other = chat
            .insert_user("bob", "human", false, None, None)
            .await
            .unwrap();
        let _ = other;
        let set_id = trace
            .create_agent_set("ops", &[spec("dev", Some(listener_user))])
            .await
            .unwrap();
        let project = trace
            .upsert_project("https://example.com/rt.git")
            .await
            .unwrap();
        trace.attach_agent_set(project, set_id).await.unwrap();
        let ws = chat.create_workstation("ws-rt", None).await.unwrap();
        chat.set_workstation_state(ws.id, "ready").await.unwrap();
        let session = chat
            .open_workstation_session(ws.id, project, Some("сессия"), listener_user)
            .await
            .unwrap();
        let runtime = AgentRuntime::new(
            LlmClient::new(),
            trace.clone(),
            chat.clone(),
            cluster(),
            CentrifugeClient::disabled(),
        );
        (
            runtime,
            listener_user,
            session.id,
            trace,
            chat,
            file,
            llm_server,
        )
    }

    async fn user_id(chat: &ChatStore, name: &str) -> i64 {
        chat.find_user_by_name(name).await.unwrap().unwrap().id
    }

    /// Сообщение `body` от `author` в чат `chat_id`.
    async fn say(chat: &ChatStore, chat_id: i64, author: i64, body: &str) -> crate::chat::Message {
        chat.send_message(chat_id, author, body, "", None, None)
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn mention_of_bound_user_triggers_agent_answering_as_that_user() {
        let (runtime, alice, chat_id, trace, chat, file, llm) = fixture("Готово").await;
        let bob = user_id(&chat, "bob").await;
        let msg = chat
            .send_message(chat_id, bob, "@alice что за проект?", "", None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(runtime.handle_message(msg.id).await);
        let messages = chat.list_messages(chat_id).await.unwrap();
        let reply = messages.last().unwrap();
        assert_ne!(reply.id, msg.id);
        // Ответ — от имени обслуживаемого пользователя.
        assert_eq!(reply.author_id, alice);
        assert_eq!(reply.body, "Готово");
        assert_eq!(reply.origin, "agent");
        // К ответу приложен артефакт, трасса задачи — в общей БД ядра.
        let artifacts = chat.list_artifacts(reply.id).await.unwrap();
        assert_eq!(artifacts.len(), 1);
        let _ = trace;
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn own_reply_of_bound_user_does_not_trigger_again() {
        let (runtime, _alice, chat_id, _trace, chat, file, llm) = fixture("@alice Готово").await;
        let bob = user_id(&chat, "bob").await;
        let msg = chat
            .send_message(chat_id, bob, "@alice вопрос", "", None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(runtime.handle_message(msg.id).await);
        let after_first = chat.list_messages(chat_id).await.unwrap().len();
        // Собственный ответ (даже с @alice внутри) своего агента не вызывает.
        let reply = chat.list_messages(chat_id).await.unwrap().pop().unwrap();
        assert!(!runtime.handle_message(reply.id).await);
        assert_eq!(
            chat.list_messages(chat_id).await.unwrap().len(),
            after_first
        );
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn message_without_mention_does_not_trigger() {
        let (runtime, alice, chat_id, _trace, chat, file, llm) = fixture("Готово").await;
        let bob = user_id(&chat, "bob").await;
        // Ни чужое сообщение без @alice, ни сообщение самой alice агента не вызывают.
        for (author, body) in [
            (bob, "Привет всем"),
            (alice, "Я тут"),
            (alice, "@alice себе"),
        ] {
            let msg = say(&chat, chat_id, author, body).await;
            assert!(!runtime.handle_message(msg.id).await, "{body}");
        }
        assert_eq!(chat.list_messages(chat_id).await.unwrap().len(), 3);
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn agent_without_binding_does_not_react() {
        let (runtime, alice, chat_id, trace, chat, file, llm) = fixture("Готово").await;
        // Пересобираем набор без привязки — состав меняется целиком.
        let set = trace
            .get_project_agent_set(
                trace
                    .upsert_project("https://example.com/rt.git")
                    .await
                    .unwrap(),
            )
            .await
            .unwrap()
            .unwrap();
        trace
            .update_agent_set(set.id, "ops", &[spec("dev", None)])
            .await
            .unwrap();
        let bob = user_id(&chat, "bob").await;
        let msg = say(&chat, chat_id, bob, "@alice есть кто?").await;
        assert!(!runtime.handle_message(msg.id).await);
        assert_eq!(chat.list_messages(chat_id).await.unwrap().len(), 1);
        let _ = alice;
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn message_in_chat_of_unattached_project_does_not_trigger() {
        let (runtime, alice, _chat_id, trace, chat, file, llm) = fixture("Готово").await;
        // Чат другого проекта, к которому набор не прикреплён.
        let other_project = trace
            .upsert_project("https://example.com/other.git")
            .await
            .unwrap();
        let ws = chat.create_workstation("ws-rt-2", None).await.unwrap();
        chat.set_workstation_state(ws.id, "ready").await.unwrap();
        let other_chat = chat
            .open_workstation_session(ws.id, other_project, None, alice)
            .await
            .unwrap();
        let bob = user_id(&chat, "bob").await;
        let msg = say(&chat, other_chat.id, bob, "@alice а здесь?").await;
        assert!(!runtime.handle_message(msg.id).await);
        assert_eq!(chat.list_messages(other_chat.id).await.unwrap().len(), 1);
        let _ = runtime;
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn message_in_general_chat_without_project_does_not_trigger() {
        let (runtime, alice, _chat_id, _trace, chat, file, llm) = fixture("Готово").await;
        let general = chat.create_chat(None, Some("общий"), alice).await.unwrap();
        let bob = user_id(&chat, "bob").await;
        let msg = say(&chat, general.id, bob, "@alice привет").await;
        assert!(!runtime.handle_message(msg.id).await);
        assert_eq!(chat.list_messages(general.id).await.unwrap().len(), 1);
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn mention_of_unknown_or_unserved_user_does_not_trigger() {
        // Упоминание имени агента (а не пользователя) или пользователя без
        // агента никого не вызывает.
        let (runtime, _alice, chat_id, _trace, chat, file, llm) = fixture("Готово").await;
        let ivan = chat
            .insert_user("ivan", "human", false, None, None)
            .await
            .unwrap();
        let msg = say(&chat, chat_id, ivan, "@dev @Agent.dev @bob помогите").await;
        assert!(!runtime.handle_message(msg.id).await);
        assert_eq!(chat.list_messages(chat_id).await.unwrap().len(), 1);
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn ask_human_posts_question_text_to_chat_and_waits() {
        let (runtime, _alice, chat_id, trace, chat, file, llm) = fixture_seq(vec![ASK_PORT]).await;
        let bob = user_id(&chat, "bob").await;
        let msg = say(&chat, chat_id, bob, "@alice подними API").await;
        assert!(runtime.handle_message(msg.id).await);
        let messages = chat.list_messages(chat_id).await.unwrap();
        let q = messages.last().unwrap();
        // В чат уходит сам вопрос, а не служебная строка с Request ID.
        assert_eq!(q.body, "Какой порт открыть?");
        assert_eq!(q.origin, "agent");
        assert!(!q.body.contains("Request ID"));
        // Запрос привязан к сообщению вопроса; задача ждёт ответа, а не «running».
        let req = trace
            .pending_for_question_message(q.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(req.agent_name, "dev");
        assert_eq!(req.chat_id, chat_id);
        let t = trace.get_trace(&req.task_id).await.unwrap().unwrap();
        assert_eq!(t.status, "waiting_human");
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn answer_to_question_resumes_agent_from_any_participant() {
        let (runtime, alice, chat_id, trace, chat, file, llm) =
            fixture_seq(vec![ASK_PORT, "Порт 8080"]).await;
        let bob = user_id(&chat, "bob").await;
        let msg = say(&chat, chat_id, bob, "@alice подними API").await;
        assert!(runtime.handle_message(msg.id).await);
        let q = chat.list_messages(chat_id).await.unwrap().pop().unwrap();
        let req = trace
            .pending_for_question_message(q.id)
            .await
            .unwrap()
            .unwrap();
        // Отвечает несвязанный участник — «ответом на сообщение-вопрос».
        let ivan = chat
            .insert_user("ivan", "human", false, None, None)
            .await
            .unwrap();
        let answer = chat
            .send_message(chat_id, ivan, "8080", "", Some(q.id), None)
            .await
            .unwrap()
            .unwrap();
        assert!(runtime.handle_message(answer.id).await);
        // Вопрос закрыт, ответ сохранён.
        assert!(trace
            .pending_for_question_message(q.id)
            .await
            .unwrap()
            .is_none());
        let t = trace.get_trace(&req.task_id).await.unwrap().unwrap();
        assert_eq!(t.status, "answered");
        // Продолжение: финальный ответ агента — от имени связанного пользователя.
        let messages = chat.list_messages(chat_id).await.unwrap();
        let last = messages.last().unwrap();
        assert_eq!(last.body, "Порт 8080");
        assert_eq!(last.origin, "agent");
        assert_eq!(last.author_id, alice);
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn thread_started_from_question_is_not_an_answer() {
        let (runtime, alice, chat_id, trace, chat, file, llm) =
            fixture_seq(vec![ASK_PORT_SHORT]).await;
        let bob = user_id(&chat, "bob").await;
        let msg = say(&chat, chat_id, bob, "@alice задача").await;
        let _ = alice;
        assert!(runtime.handle_message(msg.id).await);
        let q = chat.list_messages(chat_id).await.unwrap().pop().unwrap();
        let ivan = chat
            .insert_user("ivan", "human", false, None, None)
            .await
            .unwrap();
        // Нить от вопроса: её первое сообщение несёт parent_id на вопрос, но
        // живёт в другом чате — ответом это не считается.
        let (_thread, first) = chat
            .start_thread(
                chat_id,
                q.id,
                "Обсудим порты",
                "а какие варианты?",
                "",
                ivan,
            )
            .await
            .unwrap()
            .unwrap();
        assert!(!runtime.handle_message(first.id).await);
        assert!(trace
            .pending_for_question_message(q.id)
            .await
            .unwrap()
            .is_some());
        llm.abort();
        cleanup(&file).await;
    }

    #[test]
    fn long_command_output_is_truncated_short_is_kept() {
        assert_eq!(truncate_output("короткий вывод"), "короткий вывод");
        let long = "x".repeat(5000);
        let t = truncate_output(&long);
        assert!(t.chars().count() < long.chars().count());
        assert!(t.contains("вывод обрезан"));
        assert!(t.starts_with('x') && t.ends_with('x'));
    }

    #[tokio::test]
    async fn listen_channels_cover_each_bound_user_once() {
        let (trace, chat, file) = temp_stores().await;
        let u1 = chat
            .insert_user("a", "human", false, None, None)
            .await
            .unwrap();
        let u2 = chat
            .insert_user("b", "human", false, None, None)
            .await
            .unwrap();
        trace
            .create_agent_set(
                "ops",
                &[
                    spec("dev", Some(u1)),
                    spec("api", Some(u1)), // та же привязка — второй агент
                    spec("docs", Some(u2)),
                    spec("free", None),
                ],
            )
            .await
            .unwrap();
        let runtime = AgentRuntime::new(
            LlmClient::new(),
            trace,
            chat,
            cluster(),
            CentrifugeClient::disabled(),
        );
        // Упоминание может прийти из любого чата и нити — слушаем общий канал.
        let channels = runtime.listen_channels().await.unwrap();
        assert_eq!(channels, vec!["common".to_string()]);
        let _ = (u1, u2);
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn without_served_users_there_is_nothing_to_listen() {
        let (trace, chat, file) = temp_stores().await;
        trace
            .create_agent_set("ops", &[spec("free", None)])
            .await
            .unwrap();
        let runtime = AgentRuntime::new(
            LlmClient::new(),
            trace,
            chat,
            cluster(),
            CentrifugeClient::disabled(),
        );
        assert!(runtime.listen_channels().await.unwrap().is_empty());
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn agent_steps_do_not_trigger() {
        let (runtime, _alice, chat_id, _trace, chat, file, llm) = fixture("Готово").await;
        let bob = user_id(&chat, "bob").await;
        let call = serde_json::json!({"name": "shell", "arguments": {"command": "echo @alice"}});
        let step = chat
            .send_step(
                chat_id,
                bob,
                "echo @alice",
                "@alice",
                &call.to_string(),
                "agent",
            )
            .await
            .unwrap()
            .unwrap();
        assert!(!runtime.handle_message(step.id).await);
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn thread_started_by_served_user_triggers_on_every_message() {
        let (runtime, alice, chat_id, _trace, chat, file, llm) = fixture("Понял").await;
        let bob = user_id(&chat, "bob").await;
        let source = say(&chat, chat_id, bob, "Нужен отчёт").await;
        // Нить начала alice (как её агент инструментом start_thread).
        let (thread, first) = chat
            .start_thread(
                chat_id,
                source.id,
                "Отчёт",
                "@bob за какой период?",
                "",
                alice,
            )
            .await
            .unwrap()
            .unwrap();
        // Первое сообщение — её собственное: не вызывает.
        assert!(!runtime.handle_message(first.id).await);
        // Любое чужое сообщение в нити — без упоминания — вызывает её агента.
        let reply = say(&chat, thread.id, bob, "за сентябрь").await;
        assert!(runtime.handle_message(reply.id).await);
        let last = chat.list_messages(thread.id).await.unwrap().pop().unwrap();
        assert_eq!((last.author_id, last.body.as_str()), (alice, "Понял"));
        // В нити, начатой не ею, без упоминания — тишина.
        let (other, _) = chat
            .start_thread(chat_id, source.id, "Другое", "просто мысль", "", bob)
            .await
            .unwrap()
            .unwrap();
        let msg = say(&chat, other.id, bob, "ещё мысль").await;
        assert!(!runtime.handle_message(msg.id).await);
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn agent_reply_mentioning_other_served_user_triggers_their_agent() {
        // Агенты общаются как обычные участники: ответ одного с @другим
        // вызывает другого; одно сообщение может вызвать нескольких.
        let (runtime, users, chat_id, chat, file, llm, _max) =
            parallel_fixture(&[("dev", "alice"), ("docs", "bob")]).await;
        let (alice, bob) = (users["alice"], users["bob"]);
        let carol = chat
            .insert_user("carol", "human", false, None, None)
            .await
            .unwrap();
        let both = say(&chat, chat_id, carol, "@alice @bob оба сюда").await;
        assert!(runtime.handle_message(both.id).await);
        let authors: Vec<i64> = chat
            .list_messages(chat_id)
            .await
            .unwrap()
            .iter()
            .skip(1)
            .map(|m| m.author_id)
            .collect();
        assert_eq!(authors.len(), 2);
        assert!(authors.contains(&alice) && authors.contains(&bob));
        // Реплика alice (как её агента) с @bob вызывает агента bob.
        let from_alice = chat
            .send_message_with_origin(chat_id, alice, "@bob закоммить", "", None, None, "agent")
            .await
            .unwrap()
            .unwrap();
        assert!(runtime.handle_message(from_alice.id).await);
        let last = chat.list_messages(chat_id).await.unwrap().pop().unwrap();
        assert_eq!(last.author_id, bob);
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn agent_starts_thread_and_replies_there_trigger_it() {
        let start = r#"{"role":"assistant","content":null,"tool_calls":[{"id":"t1","type":"function","function":{"name":"start_thread","arguments":"{\"title\":\"Коммит\",\"message\":\"@bob закоммить src\"}"}}]}"#;
        let (runtime, alice, chat_id, _trace, chat, file, llm) =
            fixture_seq(vec![start, "Попросил bob в нити", "Спасибо"]).await;
        let bob = user_id(&chat, "bob").await;
        let msg = say(&chat, chat_id, bob, "@alice закончи задачу").await;
        assert!(runtime.handle_message(msg.id).await);
        // Нить начата от сообщения-триггера, от имени alice, первым сообщением — просьба.
        let thread = chat
            .list_threads(chat_id)
            .await
            .unwrap()
            .pop()
            .expect("нить не начата");
        assert_eq!(
            (thread.start_message_id, thread.created_by_id),
            (Some(msg.id), alice)
        );
        let first = chat.list_messages(thread.id).await.unwrap().remove(0);
        assert_eq!(first.title.as_deref(), Some("Коммит"));
        assert_eq!(first.body, "@bob закоммить src");
        // Ответ bob в нити снова вызывает агента alice — уже в нити.
        let done = say(&chat, thread.id, bob, "готово").await;
        assert!(runtime.handle_message(done.id).await);
        let last = chat.list_messages(thread.id).await.unwrap().pop().unwrap();
        assert_eq!((last.author_id, last.body.as_str()), (alice, "Спасибо"));
        llm.abort();
        cleanup(&file).await;
    }

    #[test]
    fn sse_frames_split_on_blank_lines() {
        let mut buffer = b"data: {\"a\":1}\n\ndata: {\"b\"".to_vec();
        let pos = find_frame_end(&buffer).unwrap();
        assert_eq!(pos, 13);
        buffer.drain(..pos + 2);
        assert_eq!(String::from_utf8_lossy(&buffer), "data: {\"b\"");
        assert_eq!(find_frame_end(&buffer), None);
    }

    // === Конфиг агента (перенесено из отменённого реактивного пути): имена
    // тестов переформулированы — триггера «упомянутый агент» больше нет,
    // конфиг резолвится для привязанного агента. ===

    #[tokio::test]
    async fn bound_agent_resolves_own_rules_and_llm() {
        let (store, _chat, file) = temp_stores().await;
        let conn_a = store
            .create_llm_connection(&crate::trace::LlmConnectionSpec {
                name: "conn-a".into(),
                api_url: "http://a/v1".into(),
                api_key: Some("key-a".into()),
                model_name: "qwen3:0.6b".into(),
                native_tools: true,
            })
            .await
            .unwrap();
        let conn_b = store
            .create_llm_connection(&crate::trace::LlmConnectionSpec {
                name: "conn-b".into(),
                api_url: "http://b/v1".into(),
                api_key: None,
                model_name: "qwen3:1b".into(),
                native_tools: true,
            })
            .await
            .unwrap();
        let set_id = store
            .create_agent_set(
                "ops",
                &[
                    AgentSpec {
                        name: "dev".to_string(),
                        description: "Правила разработчика".to_string(),
                        tools: vec!["git".to_string(), "make".to_string()],
                        max_iterations: 4,
                        llm_id: Some(conn_a),
                        parent: None,
                        skills: vec![],
                        listen_user_id: None,
                        mcp: vec![],
                    },
                    AgentSpec {
                        name: "deploy".to_string(),
                        description: "Правила деплоера".to_string(),
                        tools: vec!["docker".to_string()],
                        max_iterations: 4,
                        llm_id: Some(conn_b),
                        parent: None,
                        skills: vec![],
                        listen_user_id: None,
                        mcp: vec![],
                    },
                ],
            )
            .await
            .unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();

        // Привязанный агент получает свои правила, инструменты и LLM своего
        // подключения (url, ключ и модель).
        let (dev, _) = resolve_agent(&store, &set, "dev").await.unwrap().unwrap();
        assert_eq!(dev.prompt, "Правила разработчика");
        assert_eq!(dev.tools, vec!["git".to_string(), "make".to_string()]);
        assert_eq!(dev.llm.api_url.as_deref(), Some("http://a/v1"));
        assert_eq!(dev.llm.api_key.as_deref(), Some("key-a"));
        assert_eq!(dev.llm.model.as_deref(), Some("qwen3:0.6b"));

        let (deploy, _) = resolve_agent(&store, &set, "deploy")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(deploy.prompt, "Правила деплоера");
        assert_eq!(deploy.tools, vec!["docker".to_string()]);
        assert_eq!(deploy.llm.api_url.as_deref(), Some("http://b/v1"));
        assert!(deploy.llm.api_key.is_none());
        assert_eq!(deploy.llm.model.as_deref(), Some("qwen3:1b"));

        cleanup(&file).await;
    }

    #[tokio::test]
    async fn agent_without_connection_and_default_has_no_llm() {
        let (store, _chat, file) = temp_stores().await;
        let set_id = store
            .create_agent_set("ops", &[spec("dev", None)])
            .await
            .unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let (config, _) = resolve_agent(&store, &set, "dev").await.unwrap().unwrap();
        assert!(config.llm.api_url.is_none());
        assert!(config.llm.api_key.is_none());
        assert!(config.llm.model.is_none());
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn agent_without_connection_uses_default_llm() {
        let (store, _chat, file) = temp_stores().await;
        let default = store
            .create_llm_connection(&crate::trace::LlmConnectionSpec {
                name: "default".into(),
                api_url: "http://default/v1".into(),
                api_key: Some("default-key".into()),
                model_name: "qwen3:0.6b".into(),
                native_tools: true,
            })
            .await
            .unwrap();
        store.set_default_llm(default).await.unwrap();
        let set_id = store
            .create_agent_set("ops", &[spec("dev", None)])
            .await
            .unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let (config, _) = resolve_agent(&store, &set, "dev").await.unwrap().unwrap();
        assert_eq!(config.llm.api_url.as_deref(), Some("http://default/v1"));
        assert_eq!(config.llm.api_key.as_deref(), Some("default-key"));
        assert_eq!(config.llm.model.as_deref(), Some("qwen3:0.6b"));
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn bound_agent_applies_territory_skills_and_tools() {
        let (store, _chat, file) = temp_stores().await;
        let skill = store
            .create_capability(CapabilityKind::Skill, "review", "Формат диффов", 1, "alice")
            .await
            .unwrap();
        store
            .update_capability_content(skill, "Прогон тестов и правки", 1, "alice")
            .await
            .unwrap();

        let set_id = store
            .create_agent_set(
                "ops",
                &[
                    AgentSpec {
                        name: "src".to_string(),
                        description: "Корень проекта".to_string(),
                        tools: vec!["git".to_string(), "make".to_string()],
                        max_iterations: 3,
                        llm_id: None,
                        parent: None,
                        skills: vec![AgentCapability {
                            name: "review".to_string(),
                        }],
                        listen_user_id: None,
                        mcp: vec![],
                    },
                    AgentSpec {
                        name: "src/backend".to_string(),
                        description: "Бэкенд".to_string(),
                        tools: vec!["git".to_string(), "make".to_string()],
                        max_iterations: 3,
                        llm_id: None,
                        parent: Some("src".to_string()),
                        skills: vec![AgentCapability {
                            name: "review".to_string(),
                        }],
                        listen_user_id: None,
                        mcp: vec![],
                    },
                ],
            )
            .await
            .unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();

        let (config, territory) = resolve_agent(&store, &set, "src/backend")
            .await
            .unwrap()
            .unwrap();
        assert!(config.prompt.contains("Бэкенд"));
        assert!(config.prompt.contains("Прогон тестов и правки"));
        assert!(config.prompt.contains("review"));
        assert!(!config.prompt.contains("Формат диффов"));
        assert_eq!(config.tools, vec!["git".to_string(), "make".to_string()]);
        assert_eq!(territory.folder, "src/backend");
        cleanup(&file).await;
    }

    // === Параллелизм рантайма: разные агенты станции работают одновременно,
    // один агент — строго последовательно; redelivery не дублирует запуск. ===

    /// Мок LLM с задержкой: отвечает фиксированным текстом и считает пик
    /// одновременных in-flight запросов.
    async fn mock_llm_slow(
        answer: &'static str,
        delay_ms: u64,
    ) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        let max = Arc::new(AtomicUsize::new(0));
        let inflight = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (max2, inflight2) = (Arc::clone(&max), Arc::clone(&inflight));
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move || {
                let max = Arc::clone(&max2);
                let inflight = Arc::clone(&inflight2);
                async move {
                    let cur = inflight.fetch_add(1, Ordering::SeqCst) + 1;
                    max.fetch_max(cur, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    inflight.fetch_sub(1, Ordering::SeqCst);
                    axum::Json(serde_json::json!({
                        "choices": [{"message": {"role": "assistant", "content": answer}}]
                    }))
                }
            }),
        );
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/v1"), max, handle)
    }

    /// Фикстура параллелизма: агенты (имя, пользователь) привязаны к своим
    /// юзерам, одна ready-станция, одна сессия. Возвращает рантайм, id юзеров
    /// по имени, id чата и счётчик пика одновременных LLM-запросов.
    #[allow(clippy::type_complexity)]
    async fn parallel_fixture(
        agents: &[(&str, &str)],
    ) -> (
        AgentRuntime,
        HashMap<String, i64>,
        i64,
        ChatStore,
        std::path::PathBuf,
        tokio::task::JoinHandle<()>,
        Arc<AtomicUsize>,
    ) {
        let (trace, chat, file) = temp_stores().await;
        let (api_url, max, llm_server) = mock_llm_slow("Готово", 150).await;
        let conn = trace
            .create_llm_connection(&crate::trace::LlmConnectionSpec {
                name: "mock".into(),
                api_url,
                api_key: None,
                model_name: "m".into(),
                native_tools: true,
            })
            .await
            .unwrap();
        trace.set_default_llm(conn).await.unwrap();
        let mut users: HashMap<String, i64> = HashMap::new();
        let mut specs = Vec::new();
        for (agent_name, user_name) in agents {
            if !users.contains_key(*user_name) {
                let uid = chat
                    .insert_user(user_name, "human", false, None, None)
                    .await
                    .unwrap();
                users.insert((*user_name).to_string(), uid);
            }
            specs.push(AgentSpec {
                name: (*agent_name).to_string(),
                description: format!("Правила {agent_name}"),
                tools: vec!["git".to_string()],
                max_iterations: 1,
                llm_id: None,
                parent: None,
                skills: vec![],
                listen_user_id: users.get(*user_name).copied(),
                mcp: vec![],
            });
        }
        let set_id = trace.create_agent_set("ops", &specs).await.unwrap();
        let project = trace
            .upsert_project("https://example.com/par.git")
            .await
            .unwrap();
        trace.attach_agent_set(project, set_id).await.unwrap();
        let ws = chat.create_workstation("ws-par", None).await.unwrap();
        chat.set_workstation_state(ws.id, "ready").await.unwrap();
        let owner = *users.values().next().unwrap();
        let session = chat
            .open_workstation_session(ws.id, project, Some("сессия"), owner)
            .await
            .unwrap();
        let runtime = AgentRuntime::new(
            LlmClient::new(),
            trace,
            chat.clone(),
            cluster(),
            CentrifugeClient::disabled(),
        );
        (runtime, users, session.id, chat, file, llm_server, max)
    }

    #[tokio::test]
    async fn same_agent_two_messages_serialize() {
        let (runtime, _users, chat_id, chat, file, llm, max) =
            parallel_fixture(&[("dev", "alice")]).await;
        let carol = chat
            .insert_user("carol", "human", false, None, None)
            .await
            .unwrap();
        let m1 = say(&chat, chat_id, carol, "@alice первое").await;
        let m2 = say(&chat, chat_id, carol, "@alice второе").await;
        let (a, b) = tokio::join!(runtime.handle_message(m1.id), runtime.handle_message(m2.id));
        assert!(a && b);
        // Один агент никогда не был в двух экземплярах одновременно.
        assert_eq!(max.load(Ordering::SeqCst), 1);
        // Оба ответа записаны: вопрос, вопрос, ответ, ответ.
        assert_eq!(chat.list_messages(chat_id).await.unwrap().len(), 4);
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn different_agents_on_one_workstation_run_in_parallel() {
        let (runtime, users, chat_id, chat, file, llm, max) =
            parallel_fixture(&[("dev", "alice"), ("docs", "bob")]).await;
        let carol = chat
            .insert_user("carol", "human", false, None, None)
            .await
            .unwrap();
        let m1 = say(&chat, chat_id, carol, "@alice задача dev").await;
        let m2 = say(&chat, chat_id, carol, "@bob задача docs").await;
        let _ = users;
        let (a, b) = tokio::join!(runtime.handle_message(m1.id), runtime.handle_message(m2.id));
        assert!(a && b);
        // Разные агенты одной станции пересеклись в LLM одновременно.
        assert_eq!(max.load(Ordering::SeqCst), 2);
        assert_eq!(chat.list_messages(chat_id).await.unwrap().len(), 4);
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn redelivered_event_triggers_single_run() {
        let (runtime, _alice, chat_id, _trace, chat, file, llm) = fixture("Готово").await;
        let bob = user_id(&chat, "bob").await;
        let msg = say(&chat, chat_id, bob, "@alice дубли?").await;
        let data = serde_json::json!({"type": "message", "chat_id": chat_id, "message_id": msg.id});
        runtime.on_event(&data).await;
        runtime.on_event(&data).await; // redelivery того же message_id
        for _ in 0..100 {
            if chat.list_messages(chat_id).await.unwrap().len() >= 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        // Ровно один ответ, несмотря на два события.
        assert_eq!(chat.list_messages(chat_id).await.unwrap().len(), 2);
        llm.abort();
        cleanup(&file).await;
    }
}
