use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::agent::Agent;
use crate::chat::ChatStore;
use crate::cluster::Cluster;
use crate::config::{LlmConfig, RoleConfig};
use crate::llm::LlmClient;
use crate::scope::{territory_for, Territory};
use crate::trace::{AgentSet, TraceStore};
use crate::workstation::executor_for_workstation;

/// Запускает реактивных агентов по сообщениям чата. Команды и запуски агентов
/// одного воркстейшна сериализуются (по одному за раз).
#[derive(Clone)]
pub struct ReactiveRunner {
    llm_client: LlmClient,
    trace_store: TraceStore,
    chat_store: ChatStore,
    cluster: Cluster,
    locks: Arc<Mutex<HashMap<i64, Arc<Mutex<()>>>>>,
}

/// Собрать конфиг агента из набора: промпт = правила + данные агенту скиллы и
/// команды (единственное содержимое каталога), инструменты — отдельный список
/// без версий; территория — папка узла в дереве набора.
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
        llm: LlmConfig {
            model: agent.model.clone(),
            temperature: agent.temperature,
        },
    };
    Ok(Some((config, territory_for(set, agent))))
}

impl ReactiveRunner {
    pub fn new(
        llm_client: LlmClient,
        trace_store: TraceStore,
        chat_store: ChatStore,
        cluster: Cluster,
    ) -> Self {
        Self {
            llm_client,
            trace_store,
            chat_store,
            cluster,
            locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Поставить запуск агента для сообщения. Не блокирует: возвращает сразу.
    pub fn enqueue(&self, chat_id: i64, name: &str, agent_user_id: i64, task: String) {
        tracing::info!("reactive: enqueue agent={name} chat={chat_id}");
        let this = self.clone();
        let name = name.to_string();
        tokio::spawn(async move {
            this.run_in_workstation_queue(chat_id, &name, agent_user_id, task)
                .await;
        });
    }

    /// Найти проект чата: корневой чат (сессия) → воркстейшн → проект.
    async fn project_id_for_chat(&self, chat_id: i64) -> Option<i64> {
        let ws_id = self.chat_store.root_workstation_id(chat_id).await.ok()?;
        let ws_id = ws_id?;
        let ws = self.chat_store.get_workstation(ws_id).await.ok()??;
        Some(ws.project_id)
    }

    async fn run_in_workstation_queue(
        &self,
        chat_id: i64,
        name: &str,
        agent_user_id: i64,
        task: String,
    ) {
        // Определяем воркстейшн чата. Нет — исполняем на локальном хосте.
        let ws_id = self
            .chat_store
            .root_workstation_id(chat_id)
            .await
            .unwrap_or(None);

        let lock: Arc<Mutex<()>> = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(ws_id.unwrap_or(0))
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        let _guard = lock.lock().await;

        // Конфиг агента живёт в наборе проекта чата: ищем проект, его набор и
        // агента по имени. Нет проекта/набора/агента — агент не запускается.
        let Some(project_id) = self.project_id_for_chat(chat_id).await else {
            tracing::error!("agent {name} not run: no project for chat {chat_id}");
            return;
        };
        let Some(set) = self
            .trace_store
            .get_project_agent_set(project_id)
            .await
            .unwrap_or(None)
        else {
            tracing::error!("agent {name} not run: project {project_id} has no agent set");
            return;
        };
        let Some((role_config, territory)) = resolve_agent(&self.trace_store, &set, name)
            .await
            .unwrap_or(None)
        else {
            tracing::error!("agent {name} not found in set of project {project_id}");
            return;
        };

        let executor = executor_for_workstation(ws_id, &self.cluster);
        // Территория действует в воркстейшне (есть под/контейнер с проектом);
        // локальный запуск без воркстейшна границы не имеет.
        let scope = if ws_id.is_some() {
            Some(territory)
        } else {
            None
        };
        let agent = Agent::with_executor(
            role_config,
            self.llm_client.clone(),
            self.trace_store.clone(),
            executor,
            scope,
        );

        tracing::info!("reactive agent {name} starting for chat {chat_id}");
        let task_id = uuid::Uuid::new_v4().to_string();
        let result = match agent.run(&task_id, &task).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("reactive agent {name} failed: {e}");
                // Пишем сообщение-ошибку в чат.
                tracing::info!("reactive: posting error message for agent={name}");
                let _ = self
                    .chat_store
                    .send_message(chat_id, agent_user_id, &format!("Ошибка: {e}"), None, None)
                    .await;
                return;
            }
        };

        if self
            .chat_store
            .ensure_participant(chat_id, agent_user_id)
            .await
            .is_err()
        {
            return;
        }

        if let Ok(Some(msg)) = self
            .chat_store
            .send_message(chat_id, agent_user_id, &result, None, None)
            .await
        {
            let _ = self
                .chat_store
                .add_artifact(msg.id, "result", Some("Ответ агента"), &result)
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::{AgentCapability, AgentSpec, CapabilityKind};

    async fn store() -> (TraceStore, std::path::PathBuf) {
        let file =
            std::env::temp_dir().join(format!("aga_reactive_test_{}.db", uuid::Uuid::new_v4()));
        let store = TraceStore::new(&file.to_string_lossy()).await.unwrap();
        (store, file)
    }

    async fn cleanup(file: &std::path::PathBuf) {
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    }

    #[tokio::test]
    async fn mentioned_agent_resolves_own_rules_commands_and_llm() {
        let (store, file) = store().await;
        let set_id = store
            .create_agent_set(
                "ops",
                &[
                    AgentSpec {
                        name: "dev".to_string(),
                        description: "Правила разработчика".to_string(),
                        tools: vec!["git".to_string(), "make".to_string()],
                        max_iterations: 4,
                        model: Some("model-dev".to_string()),
                        temperature: 0.1,
                        parent: None,
                        skills: vec![],
                        commands: vec![],
                    },
                    AgentSpec {
                        name: "deploy".to_string(),
                        description: "Правила деплоера".to_string(),
                        tools: vec!["docker".to_string()],
                        max_iterations: 4,
                        model: None,
                        temperature: 0.9,
                        parent: None,
                        skills: vec![],
                        commands: vec![],
                    },
                ],
            )
            .await
            .unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();

        // В чате @Agent.<имя> запускает именно этого агента: его правила идут в
        // промпт, инструменты — в белый список исполнения, LLM — свои.
        let (dev, _) = resolve_agent(&store, &set, "dev").await.unwrap().unwrap();
        assert_eq!(dev.prompt, "Правила разработчика");
        assert_eq!(dev.tools, vec!["git".to_string(), "make".to_string()]);
        assert_eq!(dev.llm.model.as_deref(), Some("model-dev"));
        assert_eq!(dev.llm.temperature, 0.1);

        let (deploy, _) = resolve_agent(&store, &set, "deploy")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(deploy.prompt, "Правила деплоера");
        assert_eq!(deploy.tools, vec!["docker".to_string()]);
        assert!(deploy.llm.model.is_none());
        assert_eq!(deploy.llm.temperature, 0.9);

        // Несуществующего агента нет — отказа нет, просто нет конфига.
        assert!(resolve_agent(&store, &set, "nosuch")
            .await
            .unwrap()
            .is_none());
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn mentioned_agent_applies_territory_skills_commands_and_tools() {
        let (store, file) = store().await;
        // Каталог: скилл и команда с единственным текущим содержимым; правка
        // содержимого делает его актуальным (версий и фиксации нет).
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
                &[AgentSpec {
                    name: "src/backend".to_string(),
                    description: "Бэкенд".to_string(),
                    tools: vec!["git".to_string(), "make".to_string()],
                    max_iterations: 3,
                    model: None,
                    temperature: 0.7,
                    parent: None,
                    skills: vec![AgentCapability {
                        name: "review".to_string(),
                    }],
                    commands: vec![AgentCapability {
                        name: "deploy".to_string(),
                    }],
                }],
            )
            .await
            .unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();

        let (config, territory) = resolve_agent(&store, &set, "src/backend")
            .await
            .unwrap()
            .unwrap();
        // Промпт: правила + данные скилл и команда (единственное содержимое).
        assert!(config.prompt.contains("Бэкенд"));
        assert!(config.prompt.contains("Прогон тестов и правки"));
        assert!(config.prompt.contains("Выкатывать на стенд"));
        assert!(config.prompt.contains("review"));
        assert!(!config.prompt.contains("Формат диффов")); // прежнее содержимое не используется
                                                           // Инструменты — из списка агента.
        assert_eq!(config.tools, vec!["git".to_string(), "make".to_string()]);
        // Территория — папка узла в дереве набора.
        assert_eq!(territory.folder, "src/backend");
        cleanup(&file).await;
    }
}
