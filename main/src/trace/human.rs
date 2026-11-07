use super::*;
use sqlx::Row;
use uuid::Uuid;

impl TraceStore {
    pub async fn create_human_request(
        &self,
        task_id: &str,
        question: &str,
    ) -> Result<String, sqlx::Error> {
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO human_requests (id, task_id, question, status) VALUES (?, ?, ?, 'pending')")
            .bind(&id)
            .bind(task_id)
            .bind(question)
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

    /// Привязать запрос к сообщению вопроса в чате: ответом считается
    /// «ответ на это сообщение» (`parent_id`), продолжение выполняет агент
    /// `agent_name`.
    pub async fn link_human_request_chat(
        &self,
        id: &str,
        chat_id: i64,
        question_message_id: i64,
        agent_name: &str,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE human_requests SET chat_id = ?, question_message_id = ?, agent_name = ? WHERE id = ?",
        )
        .bind(chat_id)
        .bind(question_message_id)
        .bind(agent_name)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Pending-запрос, привязанный к сообщению-вопросу: по нему чат находит,
    /// что ответное сообщение — ответ агенту, а не новое задание.
    pub async fn pending_for_question_message(
        &self,
        question_message_id: i64,
    ) -> Result<Option<HumanRequest>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, task_id, question, agent_name, chat_id FROM human_requests \
             WHERE question_message_id = ? AND status = 'pending' AND chat_id IS NOT NULL",
        )
        .bind(question_message_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| HumanRequest {
            id: r.get("id"),
            task_id: r.get("task_id"),
            question: r.get("question"),
            agent_name: r.get("agent_name"),
            chat_id: r.get::<Option<i64>, _>("chat_id").unwrap_or(0),
        }))
    }

    pub async fn answer_human_request(&self, id: &str, answer: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("UPDATE human_requests SET answer = ?, status = 'answered', answered_at = CURRENT_TIMESTAMP WHERE id = ? AND status = 'pending'")
            .bind(answer)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
