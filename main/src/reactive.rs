use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::agent::Agent;
use crate::chat::ChatStore;
use crate::cluster::Cluster;
use crate::config::Config;
use crate::llm::LlmClient;
use crate::trace::TraceStore;
use crate::workstation::executor_for_workstation;
/// Запускает реактивных агентов по сообщениям чата. Команды и запуски агентов
/// одного воркстейшна сериализуются (по одному за раз).
#[derive(Clone)]
pub struct ReactiveRunner {
    config: Config,
    llm_client: LlmClient,
    trace_store: TraceStore,
    chat_store: ChatStore,
    cluster: Cluster,
    locks: Arc<Mutex<HashMap<i64, Arc<Mutex<()>>>>>,
}

impl ReactiveRunner {
    pub fn new(
        config: Config,
        llm_client: LlmClient,
        trace_store: TraceStore,
        chat_store: ChatStore,
        cluster: Cluster,
    ) -> Self {
        Self {
            config,
            llm_client,
            trace_store,
            chat_store,
            cluster,
            locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Поставить запуск агента для сообщения. Не блокирует: возвращает сразу.
    pub fn enqueue(&self, chat_id: i64, role: &str, agent_user_id: i64, task: String) {
        tracing::info!("reactive: enqueue role={role} chat={chat_id}");
        let this = self.clone();
        let role = role.to_string();
        tokio::spawn(async move {
            this.run_in_workstation_queue(chat_id, &role, agent_user_id, task)
                .await;
        });
    }

    async fn run_in_workstation_queue(
        &self,
        chat_id: i64,
        role: &str,
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

        let Some(role_config) = self.config.roles.get(role).cloned() else {
            tracing::error!("role {role} not found for reactive run");
            return;
        };

        let executor = executor_for_workstation(ws_id, &self.cluster.namespace);
        let agent = Agent::with_executor(
            role_config,
            self.llm_client.clone(),
            self.trace_store.clone(),
            executor,
        );

        tracing::info!("reactive agent {role} starting for chat {chat_id}");
        let task_id = uuid::Uuid::new_v4().to_string();
        let result = match agent.run(&task_id, &task).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("reactive agent {role} failed: {e}");
                // Пишем сообщение-ошибку в чат.
                tracing::info!("reactive: posting error message for role={role}");
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
