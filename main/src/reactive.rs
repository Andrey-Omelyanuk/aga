use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::agent::Agent;
use crate::chat::ChatStore;
use crate::cluster::Cluster;
use crate::config::{LlmConfig, RoleConfig};
use crate::llm::LlmClient;
use crate::trace::{AgentDef, AgentSet, TraceStore};
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

/// Собрать конфиг агента (правила, команды, LLM) из его определения в наборе.
pub fn role_config_from_agent(agent: &AgentDef) -> RoleConfig {
    RoleConfig {
        prompt: agent.description.clone(),
        allowed_commands: agent.allowed_commands.clone(),
        max_iterations: agent.max_iterations,
        llm: LlmConfig {
            model: agent.model.clone(),
            temperature: agent.temperature,
        },
    }
}

/// Найти агента по имени в наборе и вернуть его конфиг. Агент с таким именем
/// запускается со своими правилами и LLM, независимо от других агентов набора.
pub fn agent_role_config(set: &AgentSet, name: &str) -> Option<RoleConfig> {
    set.agents
        .iter()
        .find(|a| a.name == name)
        .map(role_config_from_agent)
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
        let Some(role_config) = agent_role_config(&set, name) else {
            tracing::error!("agent {name} not found in set of project {project_id}");
            return;
        };

        let executor = executor_for_workstation(ws_id, &self.cluster.namespace);
        let agent = Agent::with_executor(
            role_config,
            self.llm_client.clone(),
            self.trace_store.clone(),
            executor,
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

    fn agent_def(
        name: &str,
        description: &str,
        commands: &[&str],
        model: Option<&str>,
        temperature: f32,
    ) -> AgentDef {
        AgentDef {
            id: 1,
            name: name.to_string(),
            description: description.to_string(),
            allowed_commands: commands.iter().map(|s| s.to_string()).collect(),
            max_iterations: 4,
            model: model.map(|s| s.to_string()),
            temperature,
            parent_id: None,
        }
    }

    #[test]
    fn mentioned_agent_resolves_own_rules_commands_and_llm() {
        // В чате @Agent.<имя> запускает именно этого агента: его правила идёт в
        // промпт, команды — в белый список исполнения, LLM — свои.
        let set = AgentSet {
            id: 1,
            name: "ops".to_string(),
            agents: vec![
                agent_def(
                    "dev",
                    "Правила разработчика",
                    &["git", "make"],
                    Some("model-dev"),
                    0.1,
                ),
                agent_def("deploy", "Правила деплоера", &["docker"], None, 0.9),
            ],
        };
        let dev = agent_role_config(&set, "dev").unwrap();
        assert_eq!(dev.prompt, "Правила разработчика");
        assert_eq!(
            dev.allowed_commands,
            vec!["git".to_string(), "make".to_string()]
        );
        assert_eq!(dev.llm.model.as_deref(), Some("model-dev"));
        assert_eq!(dev.llm.temperature, 0.1);

        let deploy = agent_role_config(&set, "deploy").unwrap();
        assert_eq!(deploy.prompt, "Правила деплоера");
        assert_eq!(deploy.allowed_commands, vec!["docker".to_string()]);
        assert!(deploy.llm.model.is_none());
        assert_eq!(deploy.llm.temperature, 0.9);

        // Несуществующего агента нет — отказа нет запрета, просто нет конфига.
        assert!(agent_role_config(&set, "nosuch").is_none());
    }
}
