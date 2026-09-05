use crate::config::LlmConfig;
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

/// Способность из каталога (скилл или команда), данная агенту набора: ссылка
/// по имени на запись целиком. Версий нет — агент всегда берёт её последнее
/// содержимое.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentCapability {
    pub name: String,
}

/// Агент из AgentSet-а. Способности — скиллы и команды из общего каталога
/// (имя записи; агент всегда использует последнее содержимое); инструменты —
/// отдельный список исполняемого в консоли воркстейшна, без версий. LLM —
/// выбранное подключение (llm_id); своей модели и температуры у агента нет.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDef {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub max_iterations: u32,
    /// Подключение к LLM (см. llm_connections). Нет — дефолтная LLM из env.
    pub llm_id: Option<i64>,
    /// Указание на родителя в дереве набора: агент наследует под-уровень папки.
    pub parent_id: Option<i64>,
    /// Данные агенту скиллы (имя из каталога).
    pub skills: Vec<AgentCapability>,
    /// Данные агенту команды (имя из каталога).
    pub commands: Vec<AgentCapability>,
    /// Территория по узлу в дереве набора: папка узла минус папки наследников.
    pub territory: crate::scope::Territory,
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
    pub tools: Vec<String>,
    pub max_iterations: u32,
    /// Подключение к LLM (llm_connections). Нет — дефолтная LLM из env.
    pub llm_id: Option<i64>,
    pub parent: Option<String>,
    pub skills: Vec<AgentCapability>,
    pub commands: Vec<AgentCapability>,
}

/// Подключение к LLM: название, url API, ключ доступа и модель. Агент набора
/// ссылается на подключение (llm_id); одно подключение отмечается дефолтным —
/// к нему ходят агенты без своего подключения. Имя поля model_name в API —
/// «model» зарезервировано базовым классом mobx-модели веб-клиента.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConnection {
    pub id: i64,
    pub name: String,
    pub api_url: String,
    pub api_key: Option<String>,
    pub model_name: String,
    pub is_default: bool,
}

/// Спек подключения при создании/обновлении: название, url API, ключ, модель.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConnectionSpec {
    pub name: String,
    pub api_url: String,
    pub api_key: Option<String>,
    pub model_name: String,
}

/// Вид способности каталога: скилл или команда.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityKind {
    Skill,
    Command,
}

impl CapabilityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CapabilityKind::Skill => "skill",
            CapabilityKind::Command => "command",
        }
    }
}

/// Действие в истории изменений записи каталога.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityAction {
    Create,
    Update,
    Rename,
    Delete,
}

impl CapabilityAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            CapabilityAction::Create => "create",
            CapabilityAction::Update => "update",
            CapabilityAction::Rename => "rename",
            CapabilityAction::Delete => "delete",
        }
    }
}

/// Одна запись истории изменения записи каталога: кто, когда и что сделал.
/// `content` — содержимое записи после действия: по соседним записям строится
/// дифф на странице истории.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityHistoryEntry {
    pub id: i64,
    pub action: CapabilityAction,
    pub actor_id: i64,
    pub actor_name: String,
    pub created_at: DateTime<Utc>,
    pub detail: Option<String>,
    pub content: String,
}

/// Запись каталога способностей с единственным текущим содержимым.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityItem {
    pub id: i64,
    pub kind: CapabilityKind,
    pub name: String,
    pub content: String,
    /// Удалена ли запись (мягкое удаление): остаётся в списке «Удалённые».
    pub deleted: bool,
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

        // Миграция: «команды агента» переименованы в «инструменты» — отдельный
        // список исполняемого в консоли воркстейшна, версий у них нет.
        migrate_agents_tools_column(&pool).await?;

        // === Каталог способностей (скиллы и команды) ===
        // Общий на всю систему: агенты набора ссылаются на записи по имени.
        // У записи одно текущее содержимое (content); версий нет — агент всегда
        // использует последнее содержимое. Удаление — мягкое (deleted=1), чтобы
        // история изменений пережила удаление записи (запись остаётся в списке
        // «Удалённые» и её историю можно открыть). Имя уникально в пределах вида.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS capabilities (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                content TEXT NOT NULL DEFAULT '',
                deleted INTEGER NOT NULL DEFAULT 0,
                UNIQUE (kind, name)
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Миграция старых БД: единственное содержимое и мягкое удаление.
        // Простые ALTER — у старых таблиц та же UNIQUE(kind, name), конфликтов
        // пересборки нет.
        migrate_capabilities_columns(&pool).await?;

        // История изменений каталога: каждая правка (создание, изменение
        // содержимого, переименование, удаление) — отдельная запись «кто, когда
        // и что сделал». Хранится отдельно от самих записей: переживает правки
        // и удаление (удалённые записи не каскадят историю). Имя автора
        // фиксируется в момент правки — историю не исказит переименование
        // или удаление учётки.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS capability_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                capability_id INTEGER NOT NULL,
                action TEXT NOT NULL,
                actor_id INTEGER NOT NULL,
                actor_name TEXT NOT NULL DEFAULT '',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                detail TEXT,
                content TEXT NOT NULL DEFAULT '',
                FOREIGN KEY (capability_id) REFERENCES capabilities(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Миграция старых БД: содержимое записи после действия (для диффа).
        migrate_capability_history_content(&pool).await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_capability_history ON capability_history(capability_id)")
            .execute(&pool)
            .await?;

        // Данные агенту способности: имя записи каталога. Версий нет — имя
        // ссылается на запись целиком, агент берёт её последнее содержимое.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS agent_capabilities (
                agent_id INTEGER NOT NULL,
                capability_id INTEGER NOT NULL,
                PRIMARY KEY (agent_id, capability_id),
                FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE,
                FOREIGN KEY (capability_id) REFERENCES capabilities(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&pool)
        .await?;

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

        // === Подключения к LLM ===
        // Имя, url API, ключ доступа и модель. Агент набора ссылается на
        // подключение; одно подключение — дефолтное (is_default): к нему ходят
        // агенты без своего подключения. Дефолтной LLM из env больше нет.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS llm_connections (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                api_url TEXT NOT NULL,
                api_key TEXT,
                model TEXT NOT NULL DEFAULT '',
                is_default INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await?;
        // Миграция старых БД: у подключения появились модель и флаг дефолта.
        migrate_llm_connection_columns(&pool).await?;
        // Одна дефолтная LLM: частичный уникальный индекс на is_default=1.
        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_llm_connections_default
             ON llm_connections(is_default) WHERE is_default = 1",
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
                tools TEXT NOT NULL DEFAULT '[]',
                max_iterations INTEGER NOT NULL DEFAULT 3,
                llm_id INTEGER,
                parent_id INTEGER,
                UNIQUE (set_id, name),
                FOREIGN KEY (set_id) REFERENCES agent_sets(id) ON DELETE CASCADE,
                FOREIGN KEY (parent_id) REFERENCES agents(id) ON DELETE CASCADE,
                FOREIGN KEY (llm_id) REFERENCES llm_connections(id) ON DELETE SET NULL
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Миграция старых БД: у агента появилось подключение к LLM (llm_id).
        migrate_agents_llm_column(&pool).await?;

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
    /// дереве (агент папки, наследником которого становится этот агент);
    /// skills/commands — имена записей каталога (привязываются, если есть).
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
        Self::insert_agent_specs(&mut tx, set_id, specs).await?;
        tx.commit().await?;
        Ok(set_id)
    }

    /// Полностью заменить состав набора: имя и агенты с их инструментами,
    /// скиллами/командами и деревом. Возвращает false, если набора нет.
    pub async fn update_agent_set(
        &self,
        set_id: i64,
        name: &str,
        specs: &[AgentSpec],
    ) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query("UPDATE agent_sets SET name = ? WHERE id = ?")
            .bind(name)
            .bind(set_id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        // Состав заменяется целиком: старые агенты (и их способности каскадно)
        // удаляются, новые кладутся заново.
        sqlx::query("DELETE FROM agents WHERE set_id = ?")
            .bind(set_id)
            .execute(&mut *tx)
            .await?;
        Self::insert_agent_specs(&mut tx, set_id, specs).await?;
        tx.commit().await?;
        Ok(true)
    }

    async fn insert_agent_specs(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        set_id: i64,
        specs: &[AgentSpec],
    ) -> Result<(), sqlx::Error> {
        let mut ids: HashMap<String, i64> = HashMap::new();
        for spec in specs {
            let parent_id: Option<i64> = spec.parent.as_deref().and_then(|p| ids.get(p).copied());
            let tools = serde_json::to_string(&spec.tools).unwrap_or_else(|_| "[]".into());
            let result = sqlx::query(
                "INSERT INTO agents (set_id, name, description, tools, max_iterations, llm_id, parent_id)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(set_id)
            .bind(&spec.name)
            .bind(&spec.description)
            .bind(&tools)
            .bind(spec.max_iterations as i64)
            .bind(spec.llm_id)
            .bind(parent_id)
            .execute(&mut **tx)
            .await?;
            let agent_id = result.last_insert_rowid();
            ids.insert(spec.name.clone(), agent_id);

            for cap in &spec.skills {
                Self::link_capability(&mut *tx, agent_id, CapabilityKind::Skill, cap).await?;
            }
            for cap in &spec.commands {
                Self::link_capability(&mut *tx, agent_id, CapabilityKind::Command, cap).await?;
            }
        }
        Ok(())
    }

    /// Привязать способность каталога к агенту. Имени нет в каталоге — не
    /// привязываем (набор создаётся даже с отложенными ссылками).
    async fn link_capability(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        agent_id: i64,
        kind: CapabilityKind,
        cap: &AgentCapability,
    ) -> Result<(), sqlx::Error> {
        let row = sqlx::query("SELECT id FROM capabilities WHERE kind = ? AND name = ?")
            .bind(kind.as_str())
            .bind(&cap.name)
            .fetch_optional(&mut **tx)
            .await?;
        let Some(row) = row else {
            return Ok(());
        };
        let capability_id: i64 = row.get("id");
        sqlx::query("INSERT INTO agent_capabilities (agent_id, capability_id) VALUES (?, ?)")
            .bind(agent_id)
            .bind(capability_id)
            .execute(&mut **tx)
            .await?;
        Ok(())
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
            "SELECT id, name, description, tools, max_iterations, llm_id, parent_id
             FROM agents WHERE set_id = ? ORDER BY id",
        )
        .bind(set_id)
        .fetch_all(&self.pool)
        .await?;
        let mut agents = Vec::new();
        for r in rows {
            let tools_json: String = r.get("tools");
            let tools = serde_json::from_str(&tools_json).unwrap_or_else(|_| Vec::new());
            let agent_id: i64 = r.get("id");
            let caps = sqlx::query(
                "SELECT c.kind, c.name
                 FROM agent_capabilities ac JOIN capabilities c ON c.id = ac.capability_id
                 WHERE ac.agent_id = ? ORDER BY c.kind, c.name",
            )
            .bind(agent_id)
            .fetch_all(&self.pool)
            .await?;
            let mut skills = Vec::new();
            let mut commands = Vec::new();
            for cap in caps {
                let kind: String = cap.get("kind");
                let capability = AgentCapability {
                    name: cap.get("name"),
                };
                if kind == "skill" {
                    skills.push(capability);
                } else {
                    commands.push(capability);
                }
            }
            agents.push(AgentDef {
                id: agent_id,
                name: r.get("name"),
                description: r.get("description"),
                tools,
                max_iterations: r.get("max_iterations"),
                llm_id: r.get("llm_id"),
                parent_id: r.get("parent_id"),
                skills,
                commands,
                territory: Default::default(),
            });
        }
        let mut set = AgentSet {
            id: set_id,
            name,
            agents,
        };
        // Территория агента — папка его узла в дереве минус папки наследников.
        // Снимок списка — чтобы не держать set заимствованным на время правки.
        let snapshot = set.agents.clone();
        for agent in &mut set.agents {
            agent.territory = crate::scope::territory_for_list(&snapshot, agent);
        }
        Ok(Some(set))
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

    // === Методы для управления подключениями к LLM ===

    fn llm_row(&self, r: &sqlx::sqlite::SqliteRow) -> LlmConnection {
        LlmConnection {
            id: r.get("id"),
            name: r.get("name"),
            api_url: r.get("api_url"),
            api_key: r.get("api_key"),
            model_name: r.get("model"),
            is_default: r.get::<i64, _>("is_default") != 0,
        }
    }

    /// Создать подключение к LLM: название, url API, ключ доступа и модель.
    pub async fn create_llm_connection(
        &self,
        spec: &LlmConnectionSpec,
    ) -> Result<i64, sqlx::Error> {
        let result = sqlx::query(
            "INSERT INTO llm_connections (name, api_url, api_key, model) VALUES (?, ?, ?, ?)",
        )
        .bind(&spec.name)
        .bind(&spec.api_url)
        .bind(&spec.api_key)
        .bind(&spec.model_name)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Изменить подключение: название, url API, ключ, модель. Возвращает false,
    /// если подключения нет.
    pub async fn update_llm_connection(
        &self,
        id: i64,
        spec: &LlmConnectionSpec,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE llm_connections SET name = ?, api_url = ?, api_key = ?, model = ? WHERE id = ?",
        )
        .bind(&spec.name)
        .bind(&spec.api_url)
        .bind(&spec.api_key)
        .bind(&spec.model_name)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Удалить подключение. Агенты, ссылавшиеся на него, остаются без
    /// подключения (llm_id обнуляется каскадом на новых БД, на старых —
    /// ссылка просто не находится) и, как агенты без подключения, ходят к
    /// дефолтной LLM. Было дефолтным — дефолт исчезает вместе с ним.
    pub async fn delete_llm_connection(&self, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM llm_connections WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_llm_connection(&self, id: i64) -> Result<Option<LlmConnection>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, name, api_url, api_key, model, is_default
             FROM llm_connections WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| self.llm_row(&r)))
    }

    pub async fn list_llm_connections(&self) -> Result<Vec<LlmConnection>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, name, api_url, api_key, model, is_default
             FROM llm_connections ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(|r| self.llm_row(r)).collect())
    }

    /// Сделать подключение дефолтной LLM: дефолт снимается с остальных (одна
    /// дефолтная LLM). Возвращает false, если подключения нет.
    pub async fn set_default_llm(&self, id: i64) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE llm_connections SET is_default = 0 WHERE is_default = 1")
            .execute(&mut *tx)
            .await?;
        let result = sqlx::query("UPDATE llm_connections SET is_default = 1 WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    /// Снять выбор дефолтной LLM: ни одна LLM не дефолтная, агенты без своего
    /// подключения остаются без LLM.
    pub async fn clear_default_llm(&self) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE llm_connections SET is_default = 0 WHERE is_default = 1")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Дефолтная LLM (одна). None — дефолт не выбран.
    pub async fn default_llm_connection(&self) -> Result<Option<LlmConnection>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, name, api_url, api_key, model, is_default
             FROM llm_connections WHERE is_default = 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| self.llm_row(&r)))
    }

    /// LLM-конфиг агента: url, ключ и модель берутся из выбранного подключения,
    /// а без него — из дефолтной LLM (выбранной на странице «LLM»). Своей
    /// модели и температуры у агента нет — модель живёт в подключении,
    /// температура 0.7. Ни подключения, ни дефолтной LLM нет — конфиг без url:
    /// запуск агента не пройдёт (LlmClient ответит, что LLM не настроена).
    pub async fn llm_config_for(&self, agent: &AgentDef) -> Result<LlmConfig, sqlx::Error> {
        let mut llm = LlmConfig {
            model: None,
            temperature: 0.7,
            api_url: None,
            api_key: None,
        };
        // Своё подключение важнее дефолта; ссылка на удалённое — как отсутствие.
        let conn = match agent.llm_id {
            Some(id) => self.get_llm_connection(id).await?,
            None => None,
        };
        let conn = conn.or(self.default_llm_connection().await?);
        if let Some(conn) = conn {
            llm.model = Some(conn.model_name);
            llm.api_url = Some(conn.api_url);
            llm.api_key = conn.api_key;
        }
        Ok(llm)
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

    // === Каталог способностей (скиллы и команды) ===

    /// Список записей каталога одного вида: активные и (если `include_deleted`)
    /// удалённые («Удалённые»), по порядку id.
    pub async fn list_capabilities(
        &self,
        kind: CapabilityKind,
        include_deleted: bool,
    ) -> Result<Vec<CapabilityItem>, sqlx::Error> {
        let sql = if include_deleted {
            "SELECT id, kind, name, content, deleted FROM capabilities WHERE kind = ? ORDER BY id"
        } else {
            "SELECT id, kind, name, content, deleted FROM capabilities WHERE kind = ? AND deleted = 0 ORDER BY id"
        };
        let rows = sqlx::query(sql)
            .bind(kind.as_str())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(capability_from_row).collect())
    }

    /// Записи каталога с мягко удалёнными (для страницы «Удалённые»).
    pub async fn list_deleted_capabilities(
        &self,
        kind: CapabilityKind,
    ) -> Result<Vec<CapabilityItem>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, kind, name, content, deleted FROM capabilities WHERE kind = ? AND deleted = 1 ORDER BY id",
        )
        .bind(kind.as_str())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(capability_from_row).collect())
    }

    pub async fn get_capability(
        &self,
        capability_id: i64,
    ) -> Result<Option<CapabilityItem>, sqlx::Error> {
        let row =
            sqlx::query("SELECT id, kind, name, content, deleted FROM capabilities WHERE id = ?")
                .bind(capability_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| capability_from_row(&r)))
    }

    /// Занято ли имя в пределах вида (активными или удалёнными записями).
    /// Исключает сам id — правка с тем же именем не конфликтует. Удалённые
    /// записи имя занимают (UNIQUE(kind, name)), поэтому переименование в имя
    /// удалённой записи тоже запрещено.
    pub async fn capability_name_taken(
        &self,
        kind: CapabilityKind,
        name: &str,
        exclude_id: i64,
    ) -> Result<bool, sqlx::Error> {
        let row = sqlx::query(
            "SELECT COUNT(*) AS c FROM capabilities WHERE kind = ? AND name = ? AND id != ?",
        )
        .bind(kind.as_str())
        .bind(name)
        .bind(exclude_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get::<i64, _>("c") > 0)
    }

    /// Создать запись каталога с единственным текущим содержимым и записать
    /// создание в историю. Возвращает id записи.
    pub async fn create_capability(
        &self,
        kind: CapabilityKind,
        name: &str,
        content: &str,
        actor_id: i64,
        actor_name: &str,
    ) -> Result<i64, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query("INSERT INTO capabilities (kind, name, content) VALUES (?, ?, ?)")
            .bind(kind.as_str())
            .bind(name)
            .bind(content)
            .execute(&mut *tx)
            .await?;
        let id = result.last_insert_rowid();
        sqlx::query(
            "INSERT INTO capability_history (capability_id, action, actor_id, actor_name, content) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(CapabilityAction::Create.as_str())
        .bind(actor_id)
        .bind(actor_name)
        .bind(content)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Изменить содержимое записи: новое содержимое становится текущим, правка
    /// записывается в историю. Возвращает false, если записи нет.
    pub async fn update_capability_content(
        &self,
        id: i64,
        content: &str,
        actor_id: i64,
        actor_name: &str,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query("UPDATE capabilities SET content = ? WHERE id = ?")
            .bind(content)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO capability_history (capability_id, action, actor_id, actor_name, content) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(CapabilityAction::Update.as_str())
        .bind(actor_id)
        .bind(actor_name)
        .bind(content)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Переименование записи каталога. Возвращает false, если записи нет.
    pub async fn rename_capability(
        &self,
        id: i64,
        name: &str,
        actor_id: i64,
        actor_name: &str,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let content: Option<String> =
            sqlx::query_scalar("SELECT content FROM capabilities WHERE id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(content) = content else {
            tx.rollback().await?;
            return Ok(false);
        };
        let result = sqlx::query("UPDATE capabilities SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO capability_history (capability_id, action, actor_id, actor_name, detail, content) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(CapabilityAction::Rename.as_str())
        .bind(actor_id)
        .bind(actor_name)
        .bind(name)
        .bind(content)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Мягкое удаление записи: остаётся в списке «Удалённые», история
    /// сохраняется. Возвращает false, если записи нет или она уже удалена.
    pub async fn delete_capability(
        &self,
        id: i64,
        actor_id: i64,
        actor_name: &str,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let content: Option<String> =
            sqlx::query_scalar("SELECT content FROM capabilities WHERE id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(content) = content else {
            tx.rollback().await?;
            return Ok(false);
        };
        let result =
            sqlx::query("UPDATE capabilities SET deleted = 1 WHERE id = ? AND deleted = 0")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO capability_history (capability_id, action, actor_id, actor_name, content) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(CapabilityAction::Delete.as_str())
        .bind(actor_id)
        .bind(actor_name)
        .bind(content)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// История изменений записи: кто, когда и что сделал, по порядку.
    pub async fn capability_history(
        &self,
        capability_id: i64,
    ) -> Result<Vec<CapabilityHistoryEntry>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, action, actor_id, actor_name, created_at, detail, content
             FROM capability_history
             WHERE capability_id = ? ORDER BY id",
        )
        .bind(capability_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| CapabilityHistoryEntry {
                id: r.get("id"),
                action: action_from_str(&r.get::<String, _>("action"))
                    .unwrap_or(CapabilityAction::Update),
                actor_id: r.get("actor_id"),
                actor_name: r.get::<String, _>("actor_name"),
                created_at: r.get("created_at"),
                detail: r.get("detail"),
                content: r.get::<String, _>("content"),
            })
            .collect())
    }

    /// Содержимое записи каталога: единственное текущее содержимое (версий
    /// нет — агент всегда берёт последнее). Удалённая запись агенту не даётся.
    pub async fn resolve_capability(
        &self,
        kind: CapabilityKind,
        name: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT content FROM capabilities WHERE kind = ? AND name = ? AND deleted = 0",
        )
        .bind(kind.as_str())
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.get("content")))
    }

    /// Промпт агента: его правила плюс данные ему скиллы и команды, раскрытые
    /// в их единственном текущем содержимом.
    pub async fn agent_prompt(&self, agent: &AgentDef) -> Result<String, sqlx::Error> {
        let mut parts = vec![agent.description.clone()];
        for sk in &agent.skills {
            if let Some(content) = self
                .resolve_capability(CapabilityKind::Skill, &sk.name)
                .await?
            {
                parts.push(format!("Скилл «{}»: {}", sk.name, content));
            }
        }
        for cmd in &agent.commands {
            if let Some(content) = self
                .resolve_capability(CapabilityKind::Command, &cmd.name)
                .await?
            {
                parts.push(format!("Команда «{}»: {}", cmd.name, content));
            }
        }
        Ok(parts.join("\n\n"))
    }

    /// Полностью очистить таблицы трассировки/проектов/наборов и сбросить
    /// автоинкрементные счётчики (детерминированные ID после пересоздания).
    /// Используется тестовым набором (`aga seed`) — восстанавливает «чистую»
    /// БД с фиксированными идентификаторами.
    pub(crate) async fn clear_all(&self) -> Result<(), sqlx::Error> {
        // Дети раньше родителей (внешние ключи без каскада в части таблиц).
        for table in [
            "trace_entries",
            "human_requests",
            "tasks",
            "project_agent_set",
            "agent_capabilities",
            "capability_history",
            "agents",
            "agent_sets",
            "capabilities",
            "projects",
            "llm_connections",
        ] {
            sqlx::query(&format!("DELETE FROM {table}"))
                .execute(&self.pool)
                .await?;
        }
        sqlx::query("DELETE FROM sqlite_sequence")
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

/// Запись каталога из строки SELECT (id, kind, name, content, deleted).
fn capability_from_row(r: &sqlx::sqlite::SqliteRow) -> CapabilityItem {
    let kind: String = r.get("kind");
    CapabilityItem {
        id: r.get("id"),
        kind: if kind == "skill" {
            CapabilityKind::Skill
        } else {
            CapabilityKind::Command
        },
        name: r.get("name"),
        content: r.get("content"),
        deleted: r.get::<i64, _>("deleted") != 0,
    }
}

/// Действие истории по строке; неизвестное значение — правка содержимого.
fn action_from_str(s: &str) -> Option<CapabilityAction> {
    match s {
        "create" => Some(CapabilityAction::Create),
        "update" => Some(CapabilityAction::Update),
        "rename" => Some(CapabilityAction::Rename),
        "delete" => Some(CapabilityAction::Delete),
        _ => None,
    }
}

/// Миграция старых БД: история каталога хранит содержимое записи после
/// действия (`content`) — по нему страница истории строит дифф. Старые записи
/// получают пустое содержимое (снапшотов тогда не было).
async fn migrate_capability_history_content(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(capability_history)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    if !cols.iter().any(|c| c == "content") {
        sqlx::query("ALTER TABLE capability_history ADD COLUMN content TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Перевести старые БД на новую модель каталога: у записей одно содержимое
/// (`content`) и мягкое удаление (`deleted`). Содержимое берётся из последней
/// версии; UNIQUE(kind, name) в старой схеме уже есть и сохраняется.
async fn migrate_capabilities_columns(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(capabilities)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    if !cols.iter().any(|c| c == "content") {
        sqlx::query("ALTER TABLE capabilities ADD COLUMN content TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await?;
        // Наполнение из последней версии. Таблицы версий может не быть вообще
        // (старая схема без версий) — тогда остаётся пустое содержимое; если
        // версий для записи нет, COALESCE не даёт NULL нарушить NOT NULL.
        let has_versions = sqlx::query(
            "SELECT COUNT(*) AS c FROM sqlite_master WHERE type = 'table' AND name = 'capability_versions'",
        )
        .fetch_one(pool)
        .await?
        .get::<i64, _>("c")
            > 0;
        if has_versions {
            sqlx::query(
                "UPDATE capabilities SET content = COALESCE((
                    SELECT v.content FROM capability_versions v
                    WHERE v.capability_id = capabilities.id
                    ORDER BY v.id DESC LIMIT 1
                ), '')",
            )
            .execute(pool)
            .await?;
        }
    }
    if !cols.iter().any(|c| c == "deleted") {
        sqlx::query("ALTER TABLE capabilities ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0")
            .execute(pool)
            .await?;
    }
    Ok(())
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

/// Переименовать `agents.allowed_commands` в `agents.tools` у старых БД:
/// «команды агента» стали «инструментами» — отдельным списком без версий.
async fn migrate_agents_tools_column(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(agents)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    let has_allowed = cols.iter().any(|c| c == "allowed_commands");
    let has_tools = cols.iter().any(|c| c == "tools");
    if has_allowed && !has_tools {
        sqlx::query("ALTER TABLE agents RENAME COLUMN allowed_commands TO tools")
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Миграция старых БД: у агента набора появилось выбранное подключение к LLM
/// (`llm_id`). Своей модели и температуры у агента больше нет — столбцы
/// model/temperature старых БД остаются, но кодом не используются.
async fn migrate_agents_llm_column(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(agents)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    if !cols.iter().any(|c| c == "llm_id") {
        sqlx::query("ALTER TABLE agents ADD COLUMN llm_id INTEGER")
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Миграция старых БД: у подключения к LLM появились модель (`model`) и флаг
/// дефолта (`is_default`). Модель больше не из env — она живёт в подключении;
/// старые строки получают пустую модель (их правит пользователь на странице).
async fn migrate_llm_connection_columns(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(llm_connections)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    if !cols.iter().any(|c| c == "model") {
        sqlx::query("ALTER TABLE llm_connections ADD COLUMN model TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await?;
    }
    if !cols.iter().any(|c| c == "is_default") {
        sqlx::query("ALTER TABLE llm_connections ADD COLUMN is_default INTEGER NOT NULL DEFAULT 0")
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
    async fn migrates_capabilities_to_single_content_and_soft_delete() {
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
        // Старая схема: capability_versions + UNIQUE(kind, name).
        sqlx::query(
            "CREATE TABLE capabilities (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, name TEXT NOT NULL, UNIQUE (kind, name))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE capability_versions (id INTEGER PRIMARY KEY AUTOINCREMENT, capability_id INTEGER NOT NULL, version TEXT NOT NULL, content TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO capabilities (kind, name) VALUES ('skill', 'review')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO capability_versions (capability_id, version, content) VALUES (1, 'v1', 'старое содержимое')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Запись без версий: миграция не должна падать — остаётся пустое содержимое.
        sqlx::query("INSERT INTO capabilities (kind, name) VALUES ('skill', 'orphan')")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        // Открытие мигрирует: содержимое из последней версии, запись активна.
        let store = TraceStore::new(&path).await.unwrap();
        let item = store.get_capability(1).await.unwrap().unwrap();
        assert_eq!(item.content, "старое содержимое");
        assert!(!item.deleted);
        assert_eq!(item.name, "review");
        let orphan = store.get_capability(2).await.unwrap().unwrap();
        assert_eq!(orphan.content, "");
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn migrates_capability_history_content_column() {
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
        // Старая схема: история без снапшотов содержимого.
        sqlx::query(
            "CREATE TABLE capability_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                capability_id INTEGER NOT NULL,
                action TEXT NOT NULL,
                actor_id INTEGER NOT NULL,
                actor_name TEXT NOT NULL DEFAULT '',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                detail TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO capability_history (capability_id, action, actor_id, actor_name) VALUES (1, 'update', 2, 'alice')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        // Открытие добавляет столбец content: старая запись получает пустое
        // содержимое (снапшота не было) и не ломает чтение.
        let store = TraceStore::new(&path).await.unwrap();
        let history = store.capability_history(1).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].action, CapabilityAction::Update);
        assert_eq!(history[0].content, "");
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
            tools: vec!["echo".to_string()],
            max_iterations: 3,
            llm_id: None,
            parent: parent.map(|s| s.to_string()),
            skills: vec![],
            commands: vec![],
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
        let conn = store
            .create_llm_connection(&LlmConnectionSpec {
                name: "conn".to_string(),
                api_url: "http://llm/v1".to_string(),
                api_key: Some("key".to_string()),
                model_name: "qwen3:0.6b".to_string(),
            })
            .await
            .unwrap();
        let s1 = AgentSpec {
            name: "dev".to_string(),
            description: "Правила разработчика".to_string(),
            tools: vec!["git".to_string(), "make".to_string()],
            max_iterations: 2,
            llm_id: Some(conn),
            parent: None,
            skills: vec![],
            commands: vec![],
        };
        let s2 = AgentSpec {
            name: "deploy".to_string(),
            description: "Правила деплоера".to_string(),
            tools: vec!["docker".to_string()],
            max_iterations: 9,
            llm_id: None,
            parent: None,
            skills: vec![],
            commands: vec![],
        };
        let set_id = store.create_agent_set("ops", &[s1, s2]).await.unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
        let deploy = set.agents.iter().find(|a| a.name == "deploy").unwrap();
        assert_eq!(dev.description, "Правила разработчика");
        assert_eq!(dev.tools, vec!["git".to_string(), "make".to_string()]);
        assert_eq!(dev.max_iterations, 2);
        assert_eq!(dev.llm_id, Some(conn));
        assert_eq!(deploy.tools, vec!["docker".to_string()]);
        assert_eq!(deploy.max_iterations, 9);
        assert!(deploy.llm_id.is_none());
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn updated_connection_changes_url_and_key_for_agents() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let conn = store
            .create_llm_connection(&LlmConnectionSpec {
                name: "conn".to_string(),
                api_url: "http://old/v1".to_string(),
                api_key: Some("old-key".to_string()),
                model_name: "qwen3:0.6b".to_string(),
            })
            .await
            .unwrap();
        let set_id = store
            .create_agent_set(
                "ops",
                &[AgentSpec {
                    name: "dev".to_string(),
                    description: "Правила".to_string(),
                    tools: vec![],
                    max_iterations: 3,
                    llm_id: Some(conn),
                    parent: None,
                    skills: vec![],
                    commands: vec![],
                }],
            )
            .await
            .unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
        let cfg = store.llm_config_for(dev).await.unwrap();
        assert_eq!(cfg.api_url.as_deref(), Some("http://old/v1"));
        assert_eq!(cfg.api_key.as_deref(), Some("old-key"));
        assert_eq!(cfg.model.as_deref(), Some("qwen3:0.6b"));
        // Правка подключения — без перенастройки агентов: тот же llm_id, новый url/ключ.
        store
            .update_llm_connection(
                conn,
                &LlmConnectionSpec {
                    name: "conn".to_string(),
                    api_url: "http://new/v1".to_string(),
                    api_key: Some("new-key".to_string()),
                    model_name: "qwen3:1b".to_string(),
                },
            )
            .await
            .unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
        assert_eq!(dev.llm_id, Some(conn));
        let cfg = store.llm_config_for(dev).await.unwrap();
        assert_eq!(cfg.api_url.as_deref(), Some("http://new/v1"));
        assert_eq!(cfg.api_key.as_deref(), Some("new-key"));
        assert_eq!(cfg.model.as_deref(), Some("qwen3:1b"));
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn deleted_default_connection_clears_default_and_agent_has_no_llm() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let conn = store
            .create_llm_connection(&LlmConnectionSpec {
                name: "conn".to_string(),
                api_url: "http://conn/v1".to_string(),
                api_key: Some("key".to_string()),
                model_name: "qwen3:0.6b".to_string(),
            })
            .await
            .unwrap();
        store.set_default_llm(conn).await.unwrap();
        let set_id = store
            .create_agent_set(
                "ops",
                &[AgentSpec {
                    name: "dev".to_string(),
                    description: "Правила".to_string(),
                    tools: vec![],
                    max_iterations: 3,
                    llm_id: Some(conn),
                    parent: None,
                    skills: vec![],
                    commands: vec![],
                }],
            )
            .await
            .unwrap();
        store.delete_llm_connection(conn).await.unwrap();
        // Удалённое подключение исчезает из списка, дефолт сбрасывается.
        assert!(store
            .list_llm_connections()
            .await
            .unwrap()
            .iter()
            .all(|c| c.id != conn));
        assert!(store.default_llm_connection().await.unwrap().is_none());
        // Агент, его использовавший, остаётся без подключения — LLM нет.
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
        let cfg = store.llm_config_for(dev).await.unwrap();
        assert!(cfg.api_url.is_none());
        assert!(cfg.api_key.is_none());
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn agent_without_connection_uses_default_llm() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let conn = store
            .create_llm_connection(&LlmConnectionSpec {
                name: "conn".to_string(),
                api_url: "http://default/v1".to_string(),
                api_key: Some("default-key".to_string()),
                model_name: "qwen3:0.6b".to_string(),
            })
            .await
            .unwrap();
        store.set_default_llm(conn).await.unwrap();
        let set_id = store
            .create_agent_set(
                "ops",
                &[AgentSpec {
                    name: "dev".to_string(),
                    description: "Правила".to_string(),
                    tools: vec![],
                    max_iterations: 3,
                    llm_id: None,
                    parent: None,
                    skills: vec![],
                    commands: vec![],
                }],
            )
            .await
            .unwrap();
        // Агент без своего подключения ходит к дефолтной LLM: url, ключ и модель.
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
        let cfg = store.llm_config_for(dev).await.unwrap();
        assert_eq!(cfg.api_url.as_deref(), Some("http://default/v1"));
        assert_eq!(cfg.api_key.as_deref(), Some("default-key"));
        assert_eq!(cfg.model.as_deref(), Some("qwen3:0.6b"));
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn own_connection_wins_over_default() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let default = store
            .create_llm_connection(&LlmConnectionSpec {
                name: "default".to_string(),
                api_url: "http://default/v1".to_string(),
                api_key: None,
                model_name: "default-model".to_string(),
            })
            .await
            .unwrap();
        store.set_default_llm(default).await.unwrap();
        let own = store
            .create_llm_connection(&LlmConnectionSpec {
                name: "own".to_string(),
                api_url: "http://own/v1".to_string(),
                api_key: Some("own-key".to_string()),
                model_name: "own-model".to_string(),
            })
            .await
            .unwrap();
        let set_id = store
            .create_agent_set(
                "ops",
                &[AgentSpec {
                    name: "dev".to_string(),
                    description: "Правила".to_string(),
                    tools: vec![],
                    max_iterations: 3,
                    llm_id: Some(own),
                    parent: None,
                    skills: vec![],
                    commands: vec![],
                }],
            )
            .await
            .unwrap();
        // У агента со своим подключением дефолтная LLM не влияет.
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
        let cfg = store.llm_config_for(dev).await.unwrap();
        assert_eq!(cfg.api_url.as_deref(), Some("http://own/v1"));
        assert_eq!(cfg.api_key.as_deref(), Some("own-key"));
        assert_eq!(cfg.model.as_deref(), Some("own-model"));
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn setting_default_moves_it_and_clearing_removes_it() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let a = store
            .create_llm_connection(&LlmConnectionSpec {
                name: "a".to_string(),
                api_url: "http://a/v1".to_string(),
                api_key: None,
                model_name: "a-model".to_string(),
            })
            .await
            .unwrap();
        let b = store
            .create_llm_connection(&LlmConnectionSpec {
                name: "b".to_string(),
                api_url: "http://b/v1".to_string(),
                api_key: None,
                model_name: "b-model".to_string(),
            })
            .await
            .unwrap();
        assert!(store.default_llm_connection().await.unwrap().is_none());
        store.set_default_llm(a).await.unwrap();
        assert_eq!(store.default_llm_connection().await.unwrap().unwrap().id, a);
        // Новый дефолт снимает прежний: дефолтная LLM одна.
        store.set_default_llm(b).await.unwrap();
        assert_eq!(store.default_llm_connection().await.unwrap().unwrap().id, b);
        // Снять выбор: ни одна не дефолтная.
        store.clear_default_llm().await.unwrap();
        assert!(store.default_llm_connection().await.unwrap().is_none());
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn connection_keeps_model_and_default_flag() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let conn = store
            .create_llm_connection(&LlmConnectionSpec {
                name: "conn".to_string(),
                api_url: "http://llm/v1".to_string(),
                api_key: Some("key".to_string()),
                model_name: "qwen3:0.6b".to_string(),
            })
            .await
            .unwrap();
        store.set_default_llm(conn).await.unwrap();
        let listed = store.list_llm_connections().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].model_name, "qwen3:0.6b");
        assert!(listed[0].is_default);
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

    fn cap(name: &str) -> AgentCapability {
        AgentCapability {
            name: name.to_string(),
        }
    }

    #[tokio::test]
    async fn each_agent_owns_territory_by_its_tree_node() {
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
        let by_name: HashMap<&str, &AgentDef> =
            set.agents.iter().map(|a| (a.name.as_str(), a)).collect();
        // Территория агента — папка его узла в дереве набора: у папки свой
        // агент, подпапки — его наследники, их папки в его территорию не входят.
        // Корень дерева — корень проекта (папка ""): у него нет папки-родителя,
        // его территория — весь проект кроме папок наследников.
        assert_eq!(by_name["src"].territory.folder, "");
        assert_eq!(
            by_name["src"].territory.excludes,
            vec!["src/backend".to_string()]
        );
        assert_eq!(by_name["src/backend"].territory.folder, "src/backend");
        assert_eq!(
            by_name["src/backend"].territory.excludes,
            vec!["src/backend/api".to_string()]
        );
        // У листа наследников нет — территория вся его папка.
        assert_eq!(
            by_name["src/backend/api"].territory.folder,
            "src/backend/api"
        );
        assert!(by_name["src/backend/api"].territory.excludes.is_empty());
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn tools_are_plain_list_capabilities_have_single_content() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let skill = store
            .create_capability(CapabilityKind::Skill, "review", "v1", 1, "alice")
            .await
            .unwrap();
        let cmd = store
            .create_capability(CapabilityKind::Command, "deploy", "c1", 1, "alice")
            .await
            .unwrap();
        let set_id = store
            .create_agent_set(
                "ops",
                &[AgentSpec {
                    name: "dev".to_string(),
                    description: "".to_string(),
                    tools: vec!["git".to_string(), "make".to_string()],
                    max_iterations: 3,
                    llm_id: None,
                    parent: None,
                    skills: vec![cap("review")],
                    commands: vec![cap("deploy")],
                }],
            )
            .await
            .unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
        // Инструменты — плоский список без версий.
        assert_eq!(dev.tools, vec!["git".to_string(), "make".to_string()]);
        assert_eq!(dev.skills.len(), 1);
        assert_eq!(dev.skills[0].name, "review");
        assert_eq!(dev.commands[0].name, "deploy");
        // У записи каталога одно текущее содержимое — версий и фиксации нет.
        let skill_item = store.get_capability(skill).await.unwrap().unwrap();
        assert_eq!(skill_item.content, "v1");
        assert!(!skill_item.deleted);
        assert_eq!(
            store.get_capability(cmd).await.unwrap().unwrap().content,
            "c1"
        );
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn agent_uses_only_assigned_skills_and_commands() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        store
            .create_capability(
                CapabilityKind::Skill,
                "review",
                "Описание review",
                1,
                "alice",
            )
            .await
            .unwrap();
        store
            .create_capability(
                CapabilityKind::Skill,
                "polish",
                "Описание polish",
                1,
                "alice",
            )
            .await
            .unwrap();
        store
            .create_capability(
                CapabilityKind::Command,
                "deploy",
                "Команда deploy",
                1,
                "alice",
            )
            .await
            .unwrap();
        store
            .create_capability(
                CapabilityKind::Command,
                "rollback",
                "Команда rollback",
                1,
                "alice",
            )
            .await
            .unwrap();
        let set_id = store
            .create_agent_set(
                "ops",
                &[AgentSpec {
                    name: "dev".to_string(),
                    description: "Правила".to_string(),
                    tools: vec![],
                    max_iterations: 3,
                    llm_id: None,
                    parent: None,
                    skills: vec![cap("review")],
                    commands: vec![cap("deploy")],
                }],
            )
            .await
            .unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
        assert_eq!(dev.skills, vec![cap("review")]);
        assert_eq!(dev.commands, vec![cap("deploy")]);
        let prompt = store.agent_prompt(dev).await.unwrap();
        // В промпте только данные агенту способности.
        assert!(prompt.contains("Описание review"));
        assert!(prompt.contains("Команда deploy"));
        assert!(!prompt.contains("polish"));
        assert!(!prompt.contains("rollback"));
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn agent_always_uses_latest_capability_content() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let skill = store
            .create_capability(CapabilityKind::Skill, "review", "Формат диффов", 1, "alice")
            .await
            .unwrap();
        let set_id = store
            .create_agent_set(
                "ops",
                &[AgentSpec {
                    name: "dev".to_string(),
                    description: "Правила".to_string(),
                    tools: vec![],
                    max_iterations: 3,
                    llm_id: None,
                    parent: None,
                    skills: vec![cap("review")],
                    commands: vec![],
                }],
            )
            .await
            .unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
        assert!(store
            .agent_prompt(dev)
            .await
            .unwrap()
            .contains("Формат диффов"));
        // Правка содержимого подхватывается без настройки агента: версий и
        // фиксации нет — агент всегда берёт единственное текущее содержимое.
        store
            .update_capability_content(skill, "Прогон тестов и правки", 1, "alice")
            .await
            .unwrap();
        let prompt = store.agent_prompt(dev).await.unwrap();
        assert!(prompt.contains("Прогон тестов и правки"));
        assert!(!prompt.contains("Формат диффов"));
        // Удалённая запись агенту не раскрывается.
        store.delete_capability(skill, 1, "alice").await.unwrap();
        assert!(!store
            .agent_prompt(dev)
            .await
            .unwrap()
            .contains("Прогон тестов и правки"));
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn capability_actions_written_to_history_with_author() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let skill = store
            .create_capability(CapabilityKind::Skill, "review", "v1", 7, "alice")
            .await
            .unwrap();
        store
            .update_capability_content(skill, "v2", 7, "alice")
            .await
            .unwrap();
        store
            .rename_capability(skill, "review2", 8, "bob")
            .await
            .unwrap();
        store.delete_capability(skill, 9, "carol").await.unwrap();
        let history = store.capability_history(skill).await.unwrap();
        // По порядку: создание, правка содержимого, переименование, удаление —
        // каждая правка с автором и временем.
        use CapabilityAction::*;
        let actions: Vec<CapabilityAction> = history.iter().map(|h| h.action).collect();
        assert_eq!(actions, vec![Create, Update, Rename, Delete]);
        assert_eq!(history[0].actor_id, 7);
        assert_eq!(history[2].actor_id, 8);
        assert_eq!(history[3].actor_id, 9);
        assert!(history[0].created_at <= history[3].created_at);
        // Каждая запись хранит содержимое после действия — по соседним записям
        // страница истории строит дифф: create -> v1, update -> v2, rename/delete
        // содержимое не меняют.
        let contents: Vec<&str> = history.iter().map(|h| h.content.as_str()).collect();
        assert_eq!(contents, vec!["v1", "v2", "v2", "v2"]);
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn capability_name_taken_includes_active_and_deleted_but_not_self() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let skill = store
            .create_capability(CapabilityKind::Skill, "review", "v1", 1, "alice")
            .await
            .unwrap();
        // Своё же имя не считается занятым — правка с тем же именем проходит.
        assert!(!store
            .capability_name_taken(CapabilityKind::Skill, "review", skill)
            .await
            .unwrap());
        // Другая активная запись с тем же именем — занято.
        assert!(
            store
                .create_capability(CapabilityKind::Skill, "taken", "x", 1, "alice")
                .await
                .unwrap()
                > 0
        );
        assert!(store
            .capability_name_taken(CapabilityKind::Skill, "taken", skill)
            .await
            .unwrap());
        // Имя свободно в другом виде (команды не мешают скиллам).
        assert!(!store
            .capability_name_taken(CapabilityKind::Command, "taken", 0)
            .await
            .unwrap());
        // Мягко удалённая запись имя занимает.
        let taken_id = store
            .create_capability(CapabilityKind::Skill, "gone", "x", 1, "alice")
            .await
            .unwrap();
        store.delete_capability(taken_id, 1, "alice").await.unwrap();
        assert!(store
            .capability_name_taken(CapabilityKind::Skill, "gone", skill)
            .await
            .unwrap());
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn capability_renamed_and_deleted_with_history() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let skill = store
            .create_capability(CapabilityKind::Skill, "review", "v1", 1, "alice")
            .await
            .unwrap();
        let set_id = store
            .create_agent_set(
                "ops",
                &[AgentSpec {
                    name: "dev".to_string(),
                    description: "".to_string(),
                    tools: vec![],
                    max_iterations: 3,
                    llm_id: None,
                    parent: None,
                    skills: vec![cap("review")],
                    commands: vec![],
                }],
            )
            .await
            .unwrap();
        // Переименование: данные агенту сохраняются, содержимое на месте.
        assert!(store
            .rename_capability(skill, "review2", 1, "alice")
            .await
            .unwrap());
        let item = store.get_capability(skill).await.unwrap().unwrap();
        assert_eq!(item.name, "review2");
        assert_eq!(item.content, "v1");
        assert!(!item.deleted);
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
        assert_eq!(dev.skills[0].name, "review2");
        // Переименование несуществующей способности — false.
        assert!(!store.rename_capability(999, "x", 1, "alice").await.unwrap());
        // Мягкое удаление: запись остаётся в «Удалённых» с сохранённой историей.
        assert!(store.delete_capability(skill, 1, "alice").await.unwrap());
        let item = store.get_capability(skill).await.unwrap().unwrap();
        assert!(item.deleted);
        assert!(store
            .list_capabilities(CapabilityKind::Skill, false)
            .await
            .unwrap()
            .is_empty());
        let deleted = store
            .list_deleted_capabilities(CapabilityKind::Skill)
            .await
            .unwrap();
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].name, "review2");
        // История переживает удаление: создание, переименование, удаление.
        assert_eq!(store.capability_history(skill).await.unwrap().len(), 3);
        // Повторное удаление — false.
        assert!(!store.delete_capability(skill, 1, "alice").await.unwrap());
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }
}
