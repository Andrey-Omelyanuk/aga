use super::*;
use sqlx::Row;
use uuid::Uuid;

impl TraceStore {
    pub async fn create_task(&self, task_id: &str, role: &str) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO tasks (id, role, status) VALUES (?, ?, 'running')")
            .bind(task_id)
            .bind(role)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn add_entry(
        &self,
        task_id: &str,
        step: i32,
        entry_type: &str,
        content: &str,
        metadata: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO trace_entries (id, task_id, step, entry_type, content, metadata) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(task_id)
        .bind(step)
        .bind(entry_type)
        .bind(content)
        .bind(metadata)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Прибавить токены ответа LLM к задаче.
    pub async fn add_task_usage(
        &self,
        task_id: &str,
        prompt_tokens: i64,
        completion_tokens: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE tasks SET prompt_tokens = prompt_tokens + ?, completion_tokens = completion_tokens + ? WHERE id = ?",
        )
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(task_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Задачи, оставшиеся `running` после падения/рестарта агент-рантайма
    /// (единственный процесс, который их исполняет), — `interrupted`.
    /// Возвращает число таких задач. Ждущие ответа человека не трогаются.
    pub async fn interrupt_running_tasks(&self) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE tasks SET status = 'interrupted', completed_at = CURRENT_TIMESTAMP WHERE status = 'running'",
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Статусы всех задач по порядку создания (тесты).
    #[cfg(test)]
    pub async fn task_statuses(&self) -> Result<Vec<String>, sqlx::Error> {
        let rows = sqlx::query("SELECT status FROM tasks ORDER BY rowid")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|r| r.get("status")).collect())
    }

    pub async fn complete_task(&self, task_id: &str, status: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE tasks SET status = ?, completed_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(status)
            .bind(task_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Задача остановлена на вопросе человеку: ждёт ответ в чате (не «running»,
    /// иначе досрочный выход из цикла оставлял бы её запущенной навсегда).
    pub async fn wait_human_task(&self, task_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE tasks SET status = 'waiting_human' WHERE id = ?")
            .bind(task_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_trace(&self, task_id: &str) -> Result<Option<TaskTrace>, sqlx::Error> {
        let task = sqlx::query(
            "SELECT id, role, status, prompt_tokens, completion_tokens FROM tasks WHERE id = ?",
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?;

        match task {
            Some(row) => {
                let role: String = row.get("role");
                let status: String = row.get("status");

                let entries: Vec<TraceEntry> = sqlx::query_as(
                    "SELECT id, task_id, step, entry_type, content, metadata, created_at FROM trace_entries WHERE task_id = ? ORDER BY step",
                )
                .bind(task_id)
                .fetch_all(&self.pool)
                .await?;

                Ok(Some(TaskTrace {
                    task_id: task_id.to_string(),
                    role,
                    status,
                    prompt_tokens: row.get("prompt_tokens"),
                    completion_tokens: row.get("completion_tokens"),
                    entries,
                }))
            }
            None => Ok(None),
        }
    }
}
