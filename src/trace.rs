use sqlx::{SqlitePool, Row};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    pub id: String,
    pub task_id: String,
    pub step: i32,
    pub entry_type: String,
    pub content: String,
    pub metadata: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskTrace {
    pub task_id: String,
    pub role: String,
    pub status: String,
    pub entries: Vec<TraceEntry>,
}

pub struct TraceStore {
    pool: SqlitePool,
}

impl TraceStore {
    pub async fn new(db_path: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(db_path).await?;
        
        // Создаём таблицы
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                role TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                completed_at DATETIME
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS trace_entries (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                step INTEGER NOT NULL,
                entry_type TEXT NOT NULL,
                content TEXT NOT NULL,
                metadata TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (task_id) REFERENCES tasks(id)
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS human_requests (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                question TEXT NOT NULL,
                answer TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                answered_at DATETIME,
                FOREIGN KEY (task_id) REFERENCES tasks(id)
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // WAL режим для лучшей производительности
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await?;

        Ok(Self { pool })
    }

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

    pub async fn complete_task(&self, task_id: &str, status: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE tasks SET status = ?, completed_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(status)
            .bind(task_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_trace(&self, task_id: &str) -> Result<Option<TaskTrace>, sqlx::Error> {
        let task = sqlx::query("SELECT id, role, status FROM tasks WHERE id = ?")
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
                    entries,
                }))
            }
            None => Ok(None),
        }
    }

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

    pub async fn answer_human_request(
        &self,
        id: &str,
        answer: &str,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("UPDATE human_requests SET answer = ?, status = 'answered', answered_at = CURRENT_TIMESTAMP WHERE id = ? AND status = 'pending'")
            .bind(answer)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_pending_human_requests(&self) -> Result<Vec<(String, String, String)>, sqlx::Error> {
        // Возвращает (id, task_id, question)
        let rows = sqlx::query("SELECT id, task_id, question FROM human_requests WHERE status = 'pending' ORDER BY created_at")
            .fetch_all(&self.pool)
            .await?;
        
        let result: Vec<(String, String, String)> = rows
            .into_iter()
            .map(|row| {
                let id: String = row.get("id");
                let task_id: String = row.get("task_id");
                let question: String = row.get("question");
                (id, task_id, question)
            })
            .collect();
        
        Ok(result)
    }
}
