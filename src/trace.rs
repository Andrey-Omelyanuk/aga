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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub compose_path: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRole {
    pub project_id: i64,
    pub role_name: String,
    pub is_active: bool,
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

        // Таблица проектов - ключ это путь к docker compose файлу
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                compose_path TEXT NOT NULL UNIQUE,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Таблица активных ролей для проектов
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS project_roles (
                project_id INTEGER NOT NULL,
                role_name TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1,
                PRIMARY KEY (project_id, role_name),
                FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Индексы для ускорения поиска
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_project_roles_project ON project_roles(project_id)")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_project_roles_active ON project_roles(project_id, is_active)")
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

    // === Методы для управления проектами ===

    /// Создать или получить проект по пути к docker-compose файлу
    pub async fn upsert_project(&self, compose_path: &str) -> Result<i64, sqlx::Error> {
        // Пробуем найти существующий проект
        let existing = sqlx::query("SELECT id FROM projects WHERE compose_path = ?")
            .bind(compose_path)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = existing {
            let id: i64 = row.get("id");
            // Обновляем updated_at
            sqlx::query("UPDATE projects SET updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(id)
                .execute(&self.pool)
                .await?;
            Ok(id)
        } else {
            // Создаём новый проект
            let result = sqlx::query("INSERT INTO projects (compose_path) VALUES (?)")
                .bind(compose_path)
                .execute(&self.pool)
                .await?;
            Ok(result.last_insert_rowid())
        }
    }

    /// Получить проект по ID
    pub async fn get_project(&self, project_id: i64) -> Result<Option<Project>, sqlx::Error> {
        let row = sqlx::query("SELECT id, compose_path, created_at, updated_at FROM projects WHERE id = ?")
            .bind(project_id)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(r) => {
                let id: i64 = r.get("id");
                let compose_path: String = r.get("compose_path");
                let created_at: DateTime<Utc> = r.get("created_at");
                let updated_at: DateTime<Utc> = r.get("updated_at");
                Ok(Some(Project {
                    id,
                    compose_path,
                    created_at,
                    updated_at,
                }))
            }
            None => Ok(None),
        }
    }

    /// Получить проект по пути к docker-compose
    pub async fn get_project_by_path(&self, compose_path: &str) -> Result<Option<Project>, sqlx::Error> {
        let row = sqlx::query("SELECT id, compose_path, created_at, updated_at FROM projects WHERE compose_path = ?")
            .bind(compose_path)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(r) => {
                let id: i64 = r.get("id");
                let compose_path: String = r.get("compose_path");
                let created_at: DateTime<Utc> = r.get("created_at");
                let updated_at: DateTime<Utc> = r.get("updated_at");
                Ok(Some(Project {
                    id,
                    compose_path,
                    created_at,
                    updated_at,
                }))
            }
            None => Ok(None),
        }
    }

    /// Получить все проекты
    pub async fn get_all_projects(&self) -> Result<Vec<Project>, sqlx::Error> {
        let rows = sqlx::query("SELECT id, compose_path, created_at, updated_at FROM projects ORDER BY created_at")
            .fetch_all(&self.pool)
            .await?;

        let mut projects = Vec::new();
        for row in rows {
            let id: i64 = row.get("id");
            let compose_path: String = row.get("compose_path");
            let created_at: DateTime<Utc> = row.get("created_at");
            let updated_at: DateTime<Utc> = row.get("updated_at");
            projects.push(Project {
                id,
                compose_path,
                created_at,
                updated_at,
            });
        }
        Ok(projects)
    }

    /// Удалить проект
    pub async fn delete_project(&self, project_id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(project_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // === Методы для управления ролями проектов ===

    /// Активировать роль для проекта
    pub async fn activate_project_role(&self, project_id: i64, role_name: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO project_roles (project_id, role_name, is_active) VALUES (?, ?, 1)
             ON CONFLICT(project_id, role_name) DO UPDATE SET is_active = 1"
        )
        .bind(project_id)
        .bind(role_name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Деактивировать роль для проекта
    pub async fn deactivate_project_role(&self, project_id: i64, role_name: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO project_roles (project_id, role_name, is_active) VALUES (?, ?, 0)
             ON CONFLICT(project_id, role_name) DO UPDATE SET is_active = 0"
        )
        .bind(project_id)
        .bind(role_name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Получить все активные роли для проекта
    pub async fn get_active_project_roles(&self, project_id: i64) -> Result<Vec<String>, sqlx::Error> {
        let rows = sqlx::query("SELECT role_name FROM project_roles WHERE project_id = ? AND is_active = 1")
            .bind(project_id)
            .fetch_all(&self.pool)
            .await?;

        let roles: Vec<String> = rows.into_iter().map(|r| r.get("role_name")).collect();
        Ok(roles)
    }

    /// Получить все роли для проекта (активные и неактивные)
    pub async fn get_all_project_roles(&self, project_id: i64) -> Result<Vec<ProjectRole>, sqlx::Error> {
        let rows = sqlx::query("SELECT project_id, role_name, is_active FROM project_roles WHERE project_id = ?")
            .bind(project_id)
            .fetch_all(&self.pool)
            .await?;

        let roles: Vec<ProjectRole> = rows
            .into_iter()
            .map(|r| {
                let project_id: i64 = r.get("project_id");
                let role_name: String = r.get("role_name");
                let is_active: i32 = r.get("is_active");
                ProjectRole {
                    project_id,
                    role_name,
                    is_active: is_active != 0,
                }
            })
            .collect();
        Ok(roles)
    }

    /// Проверить, активна ли роль для проекта
    pub async fn is_project_role_active(&self, project_id: i64, role_name: &str) -> Result<bool, sqlx::Error> {
        let row = sqlx::query("SELECT is_active FROM project_roles WHERE project_id = ? AND role_name = ?")
            .bind(project_id)
            .bind(role_name)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(r) => {
                let is_active: i32 = r.get("is_active");
                Ok(is_active != 0)
            }
            None => Ok(false),
        }
    }

    /// Установить набор активных ролей для проекта
    pub async fn set_project_roles(&self, project_id: i64, active_roles: &[&str]) -> Result<(), sqlx::Error> {
        // Начинаем транзакцию
        let mut tx = self.pool.begin().await?;

        // Сначала деактивируем все роли
        sqlx::query("UPDATE project_roles SET is_active = 0 WHERE project_id = ?")
            .bind(project_id)
            .execute(&mut *tx)
            .await?;

        // Затем активируем указанные роли
        for role in active_roles {
            sqlx::query(
                "INSERT INTO project_roles (project_id, role_name, is_active) VALUES (?, ?, 1)
                 ON CONFLICT(project_id, role_name) DO UPDATE SET is_active = 1"
            )
            .bind(project_id)
            .bind(role)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}
