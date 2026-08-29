use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row, SqlitePool};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
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
    pub git_url: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Агент из AgentSet-а. Объединяет правила и инструменты в одном описании:
/// отдельные skills/rules/commands не выделяются — всё в `description`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDef {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub allowed_commands: Vec<String>,
    pub max_iterations: u32,
    pub model: Option<String>,
    pub temperature: f32,
    /// Указание на родителя в дереве набора: агент наследует под-уровень папки.
    pub parent_id: Option<i64>,
}

/// Набор агентов с их деревом. Прикрепляется к одному или нескольким проектам.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSet {
    pub id: i64,
    pub name: String,
    pub agents: Vec<AgentDef>,
}

/// Спек агента при создании/обновлении набора: parent — имя родителя в дереве.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub name: String,
    pub description: String,
    pub allowed_commands: Vec<String>,
    pub max_iterations: u32,
    pub model: Option<String>,
    pub temperature: f32,
    pub parent: Option<String>,
}

pub struct TraceStore {
    pool: SqlitePool,
}

impl Clone for TraceStore {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
        }
    }
}

impl TraceStore {
    pub async fn new(db_path: &str) -> Result<Self, sqlx::Error> {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let options = SqliteConnectOptions::from_str(db_path)?.create_if_missing(true);
        let pool = SqlitePoolOptions::new().connect_with(options).await?;

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

        // Таблица проектов - ключ это git-репозиторий проекта
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                git_url TEXT NOT NULL UNIQUE,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Миграция: проект раньше задавался путём к docker-compose на хосте;
        // теперь он задаётся git-URL, который воркстейшн клонирует в свой под.
        migrate_projects_git_url(&pool).await?;

        // === AgentSet (набор агентов на проект) ===
        // Роли заменены наборами: у набора свои агенты (дерево через parent_id),
        // проект хранит прикреплённый набор. Один набор — на несколько проектов.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS agent_sets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS agents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                set_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                allowed_commands TEXT NOT NULL DEFAULT '[]',
                max_iterations INTEGER NOT NULL DEFAULT 3,
                model TEXT,
                temperature REAL NOT NULL DEFAULT 0.7,
                parent_id INTEGER,
                UNIQUE (set_id, name),
                FOREIGN KEY (set_id) REFERENCES agent_sets(id) ON DELETE CASCADE,
                FOREIGN KEY (parent_id) REFERENCES agents(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS project_agent_set (
                project_id INTEGER PRIMARY KEY,
                agent_set_id INTEGER NOT NULL,
                FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
                FOREIGN KEY (agent_set_id) REFERENCES agent_sets(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Индексы для поиска агентов набора
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_agents_set ON agents(set_id)")
            .execute(&pool)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_project_agent_set ON project_agent_set(agent_set_id)",
        )
        .execute(&pool)
        .await?;

        // WAL режим для лучшей производительности
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await?;

        // Внешние ключи обязательны для каскадного удаления наборов/агентов.
        sqlx::query("PRAGMA foreign_keys=ON").execute(&pool).await?;

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

    pub async fn answer_human_request(&self, id: &str, answer: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("UPDATE human_requests SET answer = ?, status = 'answered', answered_at = CURRENT_TIMESTAMP WHERE id = ? AND status = 'pending'")
            .bind(answer)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_pending_human_requests(
        &self,
    ) -> Result<Vec<(String, String, String)>, sqlx::Error> {
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

    /// Создать или получить проект по git-URL.
    pub async fn upsert_project(&self, git_url: &str) -> Result<i64, sqlx::Error> {
        // Пробуем найти существующий проект
        let existing = sqlx::query("SELECT id FROM projects WHERE git_url = ?")
            .bind(git_url)
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
            let result = sqlx::query("INSERT INTO projects (git_url) VALUES (?)")
                .bind(git_url)
                .execute(&self.pool)
                .await?;
            Ok(result.last_insert_rowid())
        }
    }

    /// Получить проект по ID
    pub async fn get_project(&self, project_id: i64) -> Result<Option<Project>, sqlx::Error> {
        let row =
            sqlx::query("SELECT id, git_url, created_at, updated_at FROM projects WHERE id = ?")
                .bind(project_id)
                .fetch_optional(&self.pool)
                .await?;

        match row {
            Some(r) => {
                let id: i64 = r.get("id");
                let git_url: String = r.get("git_url");
                let created_at: DateTime<Utc> = r.get("created_at");
                let updated_at: DateTime<Utc> = r.get("updated_at");
                Ok(Some(Project {
                    id,
                    git_url,
                    created_at,
                    updated_at,
                }))
            }
            None => Ok(None),
        }
    }

    /// Получить проект по git-URL
    #[allow(dead_code)]
    pub async fn get_project_by_url(&self, git_url: &str) -> Result<Option<Project>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, git_url, created_at, updated_at FROM projects WHERE git_url = ?",
        )
        .bind(git_url)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => {
                let id: i64 = r.get("id");
                let git_url: String = r.get("git_url");
                let created_at: DateTime<Utc> = r.get("created_at");
                let updated_at: DateTime<Utc> = r.get("updated_at");
                Ok(Some(Project {
                    id,
                    git_url,
                    created_at,
                    updated_at,
                }))
            }
            None => Ok(None),
        }
    }

    /// Получить все проекты
    pub async fn get_all_projects(&self) -> Result<Vec<Project>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, git_url, created_at, updated_at FROM projects ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut projects = Vec::new();
        for row in rows {
            let id: i64 = row.get("id");
            let git_url: String = row.get("git_url");
            let created_at: DateTime<Utc> = row.get("created_at");
            let updated_at: DateTime<Utc> = row.get("updated_at");
            projects.push(Project {
                id,
                git_url,
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

    // === Методы для управления AgentSet-ами ===

    /// Создать набор агентов. Агенты кладутся сразу: parent — имя родителя в
    /// дереве (агент папки, наследником которого становится этот агент).
    pub async fn create_agent_set(
        &self,
        name: &str,
        specs: &[AgentSpec],
    ) -> Result<i64, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query("INSERT INTO agent_sets (name) VALUES (?)")
            .bind(name)
            .execute(&mut *tx)
            .await?;
        let set_id = result.last_insert_rowid();

        let mut ids: HashMap<String, i64> = HashMap::new();
        for spec in specs {
            let parent_id: Option<i64> = spec.parent.as_deref().and_then(|p| ids.get(p).copied());
            let cmds =
                serde_json::to_string(&spec.allowed_commands).unwrap_or_else(|_| "[]".into());
            let result = sqlx::query(
                "INSERT INTO agents (set_id, name, description, allowed_commands, max_iterations, model, temperature, parent_id)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(set_id)
            .bind(&spec.name)
            .bind(&spec.description)
            .bind(&cmds)
            .bind(spec.max_iterations as i64)
            .bind(&spec.model)
            .bind(spec.temperature as f64)
            .bind(parent_id)
            .execute(&mut *tx)
            .await?;
            ids.insert(spec.name.clone(), result.last_insert_rowid());
        }

        tx.commit().await?;
        Ok(set_id)
    }

    async fn load_set(&self, set_id: i64) -> Result<Option<AgentSet>, sqlx::Error> {
        let name_row = sqlx::query("SELECT name FROM agent_sets WHERE id = ?")
            .bind(set_id)
            .fetch_optional(&self.pool)
            .await?;
        let Some(name_row) = name_row else {
            return Ok(None);
        };
        let name: String = name_row.get("name");
        let rows = sqlx::query(
            "SELECT id, name, description, allowed_commands, max_iterations, model, temperature, parent_id
             FROM agents WHERE set_id = ? ORDER BY id",
        )
        .bind(set_id)
        .fetch_all(&self.pool)
        .await?;
        let mut agents = Vec::new();
        for r in rows {
            let cmds: String = r.get("allowed_commands");
            let allowed_commands = serde_json::from_str(&cmds).unwrap_or_else(|_| Vec::new());
            agents.push(AgentDef {
                id: r.get("id"),
                name: r.get("name"),
                description: r.get("description"),
                allowed_commands,
                max_iterations: r.get("max_iterations"),
                model: r.get("model"),
                temperature: r.get("temperature"),
                parent_id: r.get("parent_id"),
            });
        }
        Ok(Some(AgentSet {
            id: set_id,
            name,
            agents,
        }))
    }

    pub async fn list_agent_sets(&self) -> Result<Vec<AgentSet>, sqlx::Error> {
        let rows = sqlx::query("SELECT id FROM agent_sets ORDER BY id")
            .fetch_all(&self.pool)
            .await?;
        let mut sets = Vec::new();
        for r in rows {
            let id: i64 = r.get("id");
            if let Some(set) = self.load_set(id).await? {
                sets.push(set);
            }
        }
        Ok(sets)
    }

    pub async fn get_agent_set(&self, set_id: i64) -> Result<Option<AgentSet>, sqlx::Error> {
        self.load_set(set_id).await
    }

    pub async fn delete_agent_set(&self, set_id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM agent_sets WHERE id = ?")
            .bind(set_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Прикрепить набор к проекту. Один набор замещает прежний (project_id —
    /// PK): повторная привязка меняет набор проекта.
    pub async fn attach_agent_set(&self, project_id: i64, set_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO project_agent_set (project_id, agent_set_id) VALUES (?, ?)
             ON CONFLICT(project_id) DO UPDATE SET agent_set_id = excluded.agent_set_id",
        )
        .bind(project_id)
        .bind(set_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_project_agent_set(
        &self,
        project_id: i64,
    ) -> Result<Option<AgentSet>, sqlx::Error> {
        let row = sqlx::query("SELECT agent_set_id FROM project_agent_set WHERE project_id = ?")
            .bind(project_id)
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(r) => self.load_set(r.get("agent_set_id")).await,
            None => Ok(None),
        }
    }
}

/// Переименовать `projects.compose_path` в `git_url` у старых БД.
/// SQLite не поддерживает `IF EXISTS` для ALTER, поэтому столбец проверяется
/// через `PRAGMA table_info`.
async fn migrate_projects_git_url(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(projects)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    let has_compose = cols.iter().any(|c| c == "compose_path");
    let has_git = cols.iter().any(|c| c == "git_url");
    if has_compose && !has_git {
        sqlx::query("ALTER TABLE projects RENAME COLUMN compose_path TO git_url")
            .execute(pool)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path() -> (String, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("aga_trace_test_{}.db", Uuid::new_v4()));
        (path.to_string_lossy().into_owned(), path)
    }

    #[tokio::test]
    async fn project_registered_by_git_url() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let url = "git@github.com:acme/proj.git";
        let id = store.upsert_project(url).await.unwrap();
        let project = store.get_project(id).await.unwrap().unwrap();
        assert_eq!(project.git_url, url);
        let again = store.upsert_project(url).await.unwrap();
        assert_eq!(again, id);
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn migrates_compose_path_to_git_url() {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let (path, file) = temp_db_path();
        let options = SqliteConnectOptions::from_str(&path)
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE projects (id INTEGER PRIMARY KEY AUTOINCREMENT, compose_path TEXT NOT NULL UNIQUE, created_at DATETIME DEFAULT CURRENT_TIMESTAMP, updated_at DATETIME DEFAULT CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO projects (compose_path) VALUES ('/old/host/path')")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let store = TraceStore::new(&path).await.unwrap();
        let old = store.get_project(1).await.unwrap().unwrap();
        assert_eq!(old.git_url, "/old/host/path");

        let id = store
            .upsert_project("https://example.com/r.git")
            .await
            .unwrap();
        let project = store.get_project(id).await.unwrap().unwrap();
        assert_eq!(project.git_url, "https://example.com/r.git");

        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn participant_sees_all_projects() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        store
            .upsert_project("https://example.com/one.git")
            .await
            .unwrap();
        store
            .upsert_project("https://example.com/two.git")
            .await
            .unwrap();
        let projects = store.get_all_projects().await.unwrap();
        assert_eq!(projects.len(), 2);
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn created_project_visible_to_all_participants() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let id = store
            .upsert_project("https://example.com/new.git")
            .await
            .unwrap();
        // Список проектов не фильтруется по пользователю: созданный проект
        // виден сразу всем участникам.
        let projects = store.get_all_projects().await.unwrap();
        assert!(projects.iter().any(|p| p.id == id));
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    fn spec(name: &str, parent: Option<&str>) -> AgentSpec {
        AgentSpec {
            name: name.to_string(),
            description: format!("Правила {name}"),
            allowed_commands: vec!["echo".to_string()],
            max_iterations: 3,
            model: None,
            temperature: 0.7,
            parent: parent.map(|s| s.to_string()),
        }
    }

    #[tokio::test]
    async fn one_agent_set_attaches_to_many_projects() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let pa = store
            .upsert_project("https://example.com/a.git")
            .await
            .unwrap();
        let pb = store
            .upsert_project("https://example.com/b.git")
            .await
            .unwrap();
        let set_id = store
            .create_agent_set("ops", &[spec("dev", None)])
            .await
            .unwrap();
        store.attach_agent_set(pa, set_id).await.unwrap();
        store.attach_agent_set(pb, set_id).await.unwrap();
        assert_eq!(
            store.get_project_agent_set(pa).await.unwrap().unwrap().id,
            set_id
        );
        assert_eq!(
            store.get_project_agent_set(pb).await.unwrap().unwrap().id,
            set_id
        );
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn each_agent_keeps_own_rules_commands_and_llm() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let s1 = AgentSpec {
            name: "dev".to_string(),
            description: "Правила разработчика".to_string(),
            allowed_commands: vec!["git".to_string(), "make".to_string()],
            max_iterations: 2,
            model: Some("model-dev".to_string()),
            temperature: 0.1,
            parent: None,
        };
        let s2 = AgentSpec {
            name: "deploy".to_string(),
            description: "Правила деплоера".to_string(),
            allowed_commands: vec!["docker".to_string()],
            max_iterations: 9,
            model: None,
            temperature: 0.9,
            parent: None,
        };
        let set_id = store.create_agent_set("ops", &[s1, s2]).await.unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
        let deploy = set.agents.iter().find(|a| a.name == "deploy").unwrap();
        assert_eq!(dev.description, "Правила разработчика");
        assert_eq!(
            dev.allowed_commands,
            vec!["git".to_string(), "make".to_string()]
        );
        assert_eq!(dev.max_iterations, 2);
        assert_eq!(dev.model.as_deref(), Some("model-dev"));
        assert_eq!(dev.temperature, 0.1);
        assert_eq!(deploy.allowed_commands, vec!["docker".to_string()]);
        assert_eq!(deploy.max_iterations, 9);
        assert!(deploy.model.is_none());
        assert_eq!(deploy.temperature, 0.9);
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn agent_set_agents_form_tree_by_parent() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let set_id = store
            .create_agent_set(
                "tree",
                &[
                    spec("src", None),
                    spec("src/backend", Some("src")),
                    spec("src/backend/api", Some("src/backend")),
                ],
            )
            .await
            .unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let by_name: std::collections::HashMap<&str, &AgentDef> =
            set.agents.iter().map(|a| (a.name.as_str(), a)).collect();
        let root = by_name["src"];
        let backend = by_name["src/backend"];
        let api = by_name["src/backend/api"];
        // Дерево повторяет иерархию: у папки — агент, у подпапок — его наследники.
        assert_eq!(backend.parent_id, Some(root.id));
        assert_eq!(api.parent_id, Some(backend.id));
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn replacing_agent_set_changes_project_agents() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let project = store
            .upsert_project("https://example.com/r.git")
            .await
            .unwrap();
        let set_a = store
            .create_agent_set("set-a", &[spec("dev-a", None)])
            .await
            .unwrap();
        let set_b = store
            .create_agent_set("set-b", &[spec("dev-b", None)])
            .await
            .unwrap();
        store.attach_agent_set(project, set_a).await.unwrap();
        assert_eq!(
            store
                .get_project_agent_set(project)
                .await
                .unwrap()
                .unwrap()
                .name,
            "set-a"
        );
        store.attach_agent_set(project, set_b).await.unwrap();
        let set = store.get_project_agent_set(project).await.unwrap().unwrap();
        assert_eq!(set.name, "set-b");
        assert!(set.agents.iter().all(|a| a.name == "dev-b"));
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn deleted_agent_set_disappears_from_projects() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let project = store
            .upsert_project("https://example.com/r.git")
            .await
            .unwrap();
        let set_id = store
            .create_agent_set("ops", &[spec("dev", None)])
            .await
            .unwrap();
        store.attach_agent_set(project, set_id).await.unwrap();
        store.delete_agent_set(set_id).await.unwrap();
        // Набор удалён — привязка проекта каскадно снята: работает только набор.
        assert!(store
            .get_project_agent_set(project)
            .await
            .unwrap()
            .is_none());
        assert!(store.get_agent_set(set_id).await.unwrap().is_none());
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }
}
