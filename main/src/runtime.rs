//! Агент-рантайм: режим `aga agent` — отдельный процесс того же бинаря.
//!
//! Чат (HTTP-сервер `aga`) про агентов ничего не знает: он пишет сообщения
//! в БД и публикует события в Centrifugo. Этот процесс подписан на каналы
//! пользователей (`user:<id>`), которым привязаны агенты (`agents.listen_user_id`),
//! и по сообщению привязанного пользователя запускает цикл агента, отвечая
//! от его имени.
//!
//! Centrifugo — обязательная зависимость агентов: нет подписки — нет реакций
//! (чат при этом работает). Внутренней fallback-шины нет намеренно: один
//! источник событий проще.

use futures_util::StreamExt;
use serde_json::Value;

use crate::agent::Agent;
use crate::centrifuge::{parse_sse_event, user_channel, CentrifugeClient};
use crate::chat::ChatStore;
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

/// Триггерит ли сообщение с таким источником запуск агента: только набранное
/// человеком. Ответы агентов пишутся с origin 'agent' — иначе реплика от имя
/// слушаемого пользователя породила бы новый запуск (петля).
pub fn message_should_trigger(origin: &str) -> bool {
    origin == "user"
}

/// Собрать конфиг агента из набора: промпт = правила + данные агенту скиллы и
/// команды (единственное содержимое каталога), инструменты — отдельный список
/// без версий; территория — папка узла в дереве набора; LLM — выбранное
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
    };
    Ok(Some((config, crate::scope::territory_for(set, agent))))
}

pub struct AgentRuntime {
    llm_client: LlmClient,
    trace_store: TraceStore,
    chat_store: ChatStore,
    cluster: Cluster,
    centrifuge: CentrifugeClient,
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
        }
    }

    /// Каналы для подписки: по одному на каждого пользователя, к которому
    /// привязан хотя бы один агент.
    pub async fn listen_channels(&self) -> Result<Vec<String>, sqlx::Error> {
        Ok(self
            .trace_store
            .listen_user_ids()
            .await?
            .into_iter()
            .map(user_channel)
            .collect())
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
    /// События обрабатываются последовательно — единственный подписчик уже
    /// сериализует запуски, отдельная очередь на воркстейшн не нужна.
    async fn session(&self, channels: &[String]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let token = self.centrifuge.subscriber_jwt(channels)?;
        let response = self.centrifuge.sse_stream(&token).await?;
        let mut stream = response.bytes_stream();
        let mut buffer: Vec<u8> = Vec::new();
        while let Some(chunk) = stream.next().await {
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
        Ok(())
    }

    async fn on_event(&self, data: &Value) {
        if data["type"] != "message" {
            return; // lifecycle-события — дело веба, агентов они не касаются
        }
        let (Some(chat_id), Some(message_id)) = (
            data["chat_id"].as_i64(),
            data["message_id"].as_i64(),
        ) else {
            return;
        };
        self.handle_message(chat_id, message_id).await;
    }

    /// Обработка события сообщения: найти агента, привязанного к автору, чей
    /// набор прикреплён к проекту чата, — запустить и ответить от имени автора.
    /// Возвращает true, если агент отработал (ответ записан).
    pub async fn handle_message(&self, chat_id: i64, message_id: i64) -> bool {
        let msg = match self.chat_store.get_message(message_id).await {
            Ok(Some(msg)) => msg,
            Ok(None) => return false,
            Err(e) => {
                tracing::warn!("runtime: не удалось прочитать сообщение {message_id}: {e}");
                return false;
            }
        };
        if !message_should_trigger(&msg.origin) {
            return false;
        }
        let Ok(Some(project_id)) = self.chat_store.project_id_for_chat(msg.chat_id).await else {
            return false; // чат не сессия проекта — набору агентов неоткуда взяться
        };
        let Ok(Some(set)) = self.trace_store.get_project_agent_set(project_id).await else {
            return false;
        };
        let Some(agent) = self.trace_store.agent_listening_to(&set, msg.author_id) else {
            return false;
        };
        let Some((role_config, territory)) =
            resolve_agent(&self.trace_store, &set, &agent.name)
                .await
                .unwrap_or(None)
        else {
            tracing::error!("runtime: агент {} не найден в наборе {project_id}", agent.name);
            return false;
        };

        let author_id = msg.author_id;
        let task = self
            .chat_store
            .context_tail(chat_id)
            .await
            .unwrap_or_else(|| msg.body.clone());

        let ws_id = self
            .chat_store
            .root_workstation_id(chat_id)
            .await
            .unwrap_or(None);
        let executor = executor_for_workstation(ws_id, &self.cluster);
        // Территория действует в воркстейшне (есть под/контейнер с проектом);
        // локальный запуск без воркстейшна границы не имеет.
        let scope = ws_id.map(|_| territory);
        let runner = Agent::with_executor(
            role_config,
            self.llm_client.clone(),
            self.trace_store.clone(),
            executor,
            scope,
        );

        let task_id = uuid::Uuid::new_v4().to_string();
        let result = match runner.run(&task_id, &task).await {
            Ok(result) => result,
            Err(e) => {
                tracing::error!("runtime: агент {name} упал: {e}", name = agent.name);
                format!("Ошибка: {e}")
            }
        };

        let _ = self.chat_store.ensure_participant(chat_id, author_id).await;
        let Ok(Some(reply)) = self
            .chat_store
            .send_message_with_origin(chat_id, author_id, &result, "", None, None, "agent")
            .await
        else {
            return false;
        };
        let _ = self
            .chat_store
            .add_artifact(reply.id, "result", Some("Ответ агента"), &result)
            .await;
        self.centrifuge
            .publish_message(chat_id, reply.id, author_id)
            .await;
        true
    }
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
        return Err("aga agent требует настроенного Centrifugo (блок `centrifuge:` в конфиге)"
            .into());
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

    /// Мок LLM: отвечает фиксированным текстом без команд (цикл завершается
    /// сразу, воркстейшн не трогается).
    async fn mock_llm(answer: &'static str) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move || async move {
                axum::Json(serde_json::json!({
                    "choices": [{"message": {"role": "assistant", "content": answer}}]
                }))
            }),
        );
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/v1"), handle)
    }

    fn spec(name: &str, listen_user_id: Option<i64>) -> AgentSpec {
        AgentSpec {
            name: name.to_string(),
            description: format!("Правила {name}"),
            tools: vec!["git".to_string()],
            max_iterations: 3,
            llm_id: None,
            parent: None,
            skills: vec![],
            commands: vec![],
            listen_user_id,
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
        let (trace, chat, file) = temp_stores().await;
        let (api_url, llm_server) = mock_llm(answer).await;
        let conn = trace
            .create_llm_connection(&crate::trace::LlmConnectionSpec {
                name: "mock".into(),
                api_url,
                api_key: None,
                model_name: "m".into(),
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
        (runtime, listener_user, session.id, trace, chat, file, llm_server)
    }

    #[tokio::test]
    async fn bound_agent_answers_as_listening_user() {
        let (runtime, alice, chat_id, trace, chat, file, llm) = fixture("Готово").await;
        let msg = chat
            .send_message(chat_id, alice, "Что за проект?", "", None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(runtime.handle_message(chat_id, msg.id).await);
        let messages = chat.list_messages(chat_id).await.unwrap();
        let reply = messages.last().unwrap();
        assert_ne!(reply.id, msg.id);
        // Ответ — от имени слушаемого пользователя, помеченный как нечеловеческий.
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
        let (runtime, alice, chat_id, _trace, chat, file, llm) = fixture("Готово").await;
        let msg = chat
            .send_message(chat_id, alice, "Вопрос", "", None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(runtime.handle_message(chat_id, msg.id).await);
        let after_first = chat.list_messages(chat_id).await.unwrap().len();
        // Событие о собственном ответе (тот же автор) — реакции нет.
        let reply = chat.list_messages(chat_id).await.unwrap().pop().unwrap();
        assert!(!runtime.handle_message(chat_id, reply.id).await);
        assert_eq!(chat.list_messages(chat_id).await.unwrap().len(), after_first);
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn message_of_user_without_binding_does_not_trigger() {
        let (runtime, _alice, chat_id, trace, chat, file, llm) = fixture("Готово").await;
        let bob = chat
            .insert_user("karl", "human", false, None, None)
            .await
            .unwrap();
        let msg = chat
            .send_message(chat_id, bob, "Привет от Карла", "", None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(!runtime.handle_message(chat_id, msg.id).await);
        // Никто не ответил.
        assert_eq!(chat.list_messages(chat_id).await.unwrap().len(), 1);
        let _ = trace;
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn agent_without_binding_does_not_react() {
        let (runtime, alice, chat_id, trace, chat, file, llm) = fixture("Готово").await;
        // Пересобираем набор без привязки — состав меняется целиком.
        let set = trace.get_project_agent_set(
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
        let msg = chat
            .send_message(chat_id, alice, "Есть кто?", "", None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(!runtime.handle_message(chat_id, msg.id).await);
        assert_eq!(chat.list_messages(chat_id).await.unwrap().len(), 1);
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
        let msg = chat
            .send_message(other_chat.id, alice, "А здесь?", "", None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(!runtime.handle_message(other_chat.id, msg.id).await);
        assert_eq!(chat.list_messages(other_chat.id).await.unwrap().len(), 1);
        let _ = runtime;
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn message_in_general_chat_without_project_does_not_trigger() {
        let (runtime, alice, _chat_id, _trace, chat, file, llm) = fixture("Готово").await;
        let general = chat.create_chat(None, Some("общий"), alice).await.unwrap();
        let msg = chat
            .send_message(general.id, alice, "Привет всем", "", None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(!runtime.handle_message(general.id, msg.id).await);
        assert_eq!(chat.list_messages(general.id).await.unwrap().len(), 1);
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn mention_by_name_no_longer_triggers_runtime() {
        // Текст с @Agent.<имя> для рантайма — обычное сообщение: без привязки
        // к автору реакции нет (отменённый триггер не работает и здесь).
        let (runtime, _alice, chat_id, _trace, chat, file, llm) = fixture("Готово").await;
        let ivan = chat
            .insert_user("ivan", "human", false, None, None)
            .await
            .unwrap();
        let msg = chat
            .send_message(chat_id, ivan, "@Agent.dev помоги", "", None, None)
            .await
            .unwrap()
            .unwrap();
        assert!(!runtime.handle_message(chat_id, msg.id).await);
        assert_eq!(chat.list_messages(chat_id).await.unwrap().len(), 1);
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn listen_channels_cover_each_bound_user_once() {
        let (trace, chat, file) = temp_stores().await;
        let u1 = chat.insert_user("a", "human", false, None, None).await.unwrap();
        let u2 = chat.insert_user("b", "human", false, None, None).await.unwrap();
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
        let channels = runtime.listen_channels().await.unwrap();
        assert_eq!(channels, vec![user_channel(u1), user_channel(u2)]);
        cleanup(&file).await;
    }

    #[test]
    fn only_messages_authored_by_people_trigger() {
        assert!(message_should_trigger("user"));
        assert!(!message_should_trigger("agent"));
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
    async fn bound_agent_resolves_own_rules_commands_and_llm() {
        let (store, _chat, file) = temp_stores().await;
        let conn_a = store
            .create_llm_connection(&crate::trace::LlmConnectionSpec {
                name: "conn-a".into(),
                api_url: "http://a/v1".into(),
                api_key: Some("key-a".into()),
                model_name: "qwen3:0.6b".into(),
            })
            .await
            .unwrap();
        let conn_b = store
            .create_llm_connection(&crate::trace::LlmConnectionSpec {
                name: "conn-b".into(),
                api_url: "http://b/v1".into(),
                api_key: None,
                model_name: "qwen3:1b".into(),
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
                        commands: vec![],
                        listen_user_id: None,
                    },
                    AgentSpec {
                        name: "deploy".to_string(),
                        description: "Правила деплоера".to_string(),
                        tools: vec!["docker".to_string()],
                        max_iterations: 4,
                        llm_id: Some(conn_b),
                        parent: None,
                        skills: vec![],
                        commands: vec![],
                        listen_user_id: None,
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
    async fn bound_agent_applies_territory_skills_commands_and_tools() {
        let (store, _chat, file) = temp_stores().await;
        let skill = store
            .create_capability(CapabilityKind::Skill, "review", "Формат диффов", 1, "alice")
            .await
            .unwrap();
        store
            .update_capability_content(skill, "Прогон тестов и правки", 1, "alice")
            .await
            .unwrap();
        store
            .create_capability(
                CapabilityKind::Command,
                "deploy",
                "Выкатывать на стенд",
                1,
                "alice",
            )
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
                        commands: vec![AgentCapability {
                            name: "deploy".to_string(),
                        }],
                        listen_user_id: None,
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
                        commands: vec![AgentCapability {
                            name: "deploy".to_string(),
                        }],
                        listen_user_id: None,
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
        assert!(config.prompt.contains("Выкатывать на стенд"));
        assert!(config.prompt.contains("review"));
        assert!(!config.prompt.contains("Формат диффов"));
        assert_eq!(config.tools, vec!["git".to_string(), "make".to_string()]);
        assert_eq!(territory.folder, "src/backend");
        cleanup(&file).await;
    }
}
