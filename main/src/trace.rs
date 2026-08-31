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

/// Способность из каталога (скилл или команда), данная агенту набора:
/// версия не зафиксирована (последняя) или зафиксирована точно.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentCapability {
    pub name: String,
    pub pinned_version: Option<String>,
}

/// Агент из AgentSet-а. Способности — скиллы и команды из общего каталога
/// с версиями (данные агенту, с возможной фиксацией версии); инструменты —
/// отдельный список исполняемого в консоли воркстейшна, без версий.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDef {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub max_iterations: u32,
    pub model: Option<String>,
    pub temperature: f32,
    /// Указание на родителя в дереве набора: агент наследует под-уровень папки.
    pub parent_id: Option<i64>,
    /// Данные агенту скиллы (имя из каталога + зафиксированная версия).
    pub skills: Vec<AgentCapability>,
    /// Данные агенту команды (имя из каталога + зафиксированная версия).
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
    pub model: Option<String>,
    pub temperature: f32,
    pub parent: Option<String>,
    pub skills: Vec<AgentCapability>,
    pub commands: Vec<AgentCapability>,
}

/// Вид способности каталога: скилл или команда. Версии есть у обоих.
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

/// Одна версия способности каталога.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityVersion {
    pub version: String,
    pub content: String,
}

/// Запись каталога способностей со всеми её версиями.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityItem {
    pub id: i64,
    pub name: String,
    pub versions: Vec<CapabilityVersion>,
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

        // === Каталог способностей (скиллы и команды с версиями) ===
        // Общий на всю систему: агенты набора ссылаются на записи по имени и
        // могут зафиксировать версию; без фиксации берётся последняя.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS capabilities (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                UNIQUE (kind, name)
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS capability_versions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                capability_id INTEGER NOT NULL,
                version TEXT NOT NULL,
                content TEXT NOT NULL,
                UNIQUE (capability_id, version),
                FOREIGN KEY (capability_id) REFERENCES capabilities(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS agent_capabilities (
                agent_id INTEGER NOT NULL,
                capability_id INTEGER NOT NULL,
                pinned_version TEXT,
                PRIMARY KEY (agent_id, capability_id),
                FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE,
                FOREIGN KEY (capability_id) REFERENCES capabilities(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_capability_versions ON capability_versions(capability_id)")
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

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS agents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                set_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                tools TEXT NOT NULL DEFAULT '[]',
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
                "INSERT INTO agents (set_id, name, description, tools, max_iterations, model, temperature, parent_id)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(set_id)
            .bind(&spec.name)
            .bind(&spec.description)
            .bind(&tools)
            .bind(spec.max_iterations as i64)
            .bind(&spec.model)
            .bind(spec.temperature as f64)
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
        sqlx::query(
            "INSERT INTO agent_capabilities (agent_id, capability_id, pinned_version) VALUES (?, ?, ?)",
        )
        .bind(agent_id)
        .bind(capability_id)
        .bind(&cap.pinned_version)
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
            "SELECT id, name, description, tools, max_iterations, model, temperature, parent_id
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
                "SELECT c.kind, c.name, ac.pinned_version
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
                    pinned_version: cap.get("pinned_version"),
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
                model: r.get("model"),
                temperature: r.get("temperature"),
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

    pub async fn list_capabilities(
        &self,
        kind: CapabilityKind,
    ) -> Result<Vec<CapabilityItem>, sqlx::Error> {
        let rows = sqlx::query("SELECT id, name FROM capabilities WHERE kind = ? ORDER BY id")
            .bind(kind.as_str())
            .fetch_all(&self.pool)
            .await?;
        let mut items = Vec::new();
        for r in rows {
            let id: i64 = r.get("id");
            items.push(self.load_capability(id).await?.expect("row exists"));
        }
        Ok(items)
    }

    pub async fn get_capability(
        &self,
        capability_id: i64,
    ) -> Result<Option<CapabilityItem>, sqlx::Error> {
        self.load_capability(capability_id).await
    }

    async fn load_capability(
        &self,
        capability_id: i64,
    ) -> Result<Option<CapabilityItem>, sqlx::Error> {
        let row = sqlx::query("SELECT id, name FROM capabilities WHERE id = ?")
            .bind(capability_id)
            .fetch_optional(&self.pool)
            .await?;
        let Some(r) = row else {
            return Ok(None);
        };
        let versions = sqlx::query(
            "SELECT version, content FROM capability_versions WHERE capability_id = ? ORDER BY id",
        )
        .bind(capability_id)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(|v| CapabilityVersion {
            version: v.get("version"),
            content: v.get("content"),
        })
        .collect();
        Ok(Some(CapabilityItem {
            id: capability_id,
            name: r.get("name"),
            versions,
        }))
    }

    pub async fn create_capability(
        &self,
        kind: CapabilityKind,
        name: &str,
        versions: &[CapabilityVersion],
    ) -> Result<i64, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query("INSERT INTO capabilities (kind, name) VALUES (?, ?)")
            .bind(kind.as_str())
            .bind(name)
            .execute(&mut *tx)
            .await?;
        let id = result.last_insert_rowid();
        for v in versions {
            sqlx::query(
                "INSERT INTO capability_versions (capability_id, version, content) VALUES (?, ?, ?)",
            )
            .bind(id)
            .bind(&v.version)
            .bind(&v.content)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(id)
    }

    pub async fn add_capability_version(
        &self,
        capability_id: i64,
        version: &str,
        content: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO capability_versions (capability_id, version, content) VALUES (?, ?, ?)",
        )
        .bind(capability_id)
        .bind(version)
        .bind(content)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Содержимое способности каталога: зафиксированная версия или последняя
    /// (добавленная позже всех). Версия — текст; «последняя» — max id версии.
    pub async fn resolve_capability(
        &self,
        kind: CapabilityKind,
        name: &str,
        pinned: Option<&str>,
    ) -> Result<Option<String>, sqlx::Error> {
        let row = match pinned {
            Some(version) => {
                sqlx::query(
                    "SELECT v.content FROM capability_versions v
                 JOIN capabilities c ON c.id = v.capability_id
                 WHERE c.kind = ? AND c.name = ? AND v.version = ?",
                )
                .bind(kind.as_str())
                .bind(name)
                .bind(version)
                .fetch_optional(&self.pool)
                .await?
            }
            None => {
                sqlx::query(
                    "SELECT v.content FROM capability_versions v
                 JOIN capabilities c ON c.id = v.capability_id
                 WHERE c.kind = ? AND c.name = ?
                 ORDER BY v.id DESC LIMIT 1",
                )
                .bind(kind.as_str())
                .bind(name)
                .fetch_optional(&self.pool)
                .await?
            }
        };
        Ok(row.map(|r| r.get("content")))
    }

    /// Промпт агента: его правила плюс данные ему скиллы и команды, раскрытые
    /// в зафиксированной или последней версии.
    pub async fn agent_prompt(&self, agent: &AgentDef) -> Result<String, sqlx::Error> {
        let mut parts = vec![agent.description.clone()];
        for sk in &agent.skills {
            if let Some(content) = self
                .resolve_capability(
                    CapabilityKind::Skill,
                    &sk.name,
                    sk.pinned_version.as_deref(),
                )
                .await?
            {
                parts.push(format!("Скилл «{}»: {}", sk.name, content));
            }
        }
        for cmd in &agent.commands {
            if let Some(content) = self
                .resolve_capability(
                    CapabilityKind::Command,
                    &cmd.name,
                    cmd.pinned_version.as_deref(),
                )
                .await?
            {
                parts.push(format!("Команда «{}»: {}", cmd.name, content));
            }
        }
        Ok(parts.join("\n\n"))
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
            tools: vec!["echo".to_string()],
            max_iterations: 3,
            model: None,
            temperature: 0.7,
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
        let s1 = AgentSpec {
            name: "dev".to_string(),
            description: "Правила разработчика".to_string(),
            tools: vec!["git".to_string(), "make".to_string()],
            max_iterations: 2,
            model: Some("model-dev".to_string()),
            temperature: 0.1,
            parent: None,
            skills: vec![],
            commands: vec![],
        };
        let s2 = AgentSpec {
            name: "deploy".to_string(),
            description: "Правила деплоера".to_string(),
            tools: vec!["docker".to_string()],
            max_iterations: 9,
            model: None,
            temperature: 0.9,
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
        assert_eq!(dev.model.as_deref(), Some("model-dev"));
        assert_eq!(dev.temperature, 0.1);
        assert_eq!(deploy.tools, vec!["docker".to_string()]);
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

    fn cap(name: &str, pinned: Option<&str>) -> AgentCapability {
        AgentCapability {
            name: name.to_string(),
            pinned_version: pinned.map(|s| s.to_string()),
        }
    }

    fn version(v: &str, content: &str) -> CapabilityVersion {
        CapabilityVersion {
            version: v.to_string(),
            content: content.to_string(),
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
        assert_eq!(by_name["src"].territory.folder, "src");
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
    async fn tools_have_no_versions_only_skills_and_commands_do() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let skill = store
            .create_capability(
                CapabilityKind::Skill,
                "review",
                &[version("1", "v1"), version("2", "v2")],
            )
            .await
            .unwrap();
        let cmd = store
            .create_capability(CapabilityKind::Command, "deploy", &[version("1", "c1")])
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
                    model: None,
                    temperature: 0.7,
                    parent: None,
                    skills: vec![cap("review", None)],
                    commands: vec![cap("deploy", None)],
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
        assert!(dev.skills[0].pinned_version.is_none());
        assert_eq!(dev.commands[0].name, "deploy");
        // Версии живут в каталоге — у скиллов и команд.
        assert_eq!(
            store
                .get_capability(skill)
                .await
                .unwrap()
                .unwrap()
                .versions
                .len(),
            2
        );
        assert_eq!(
            store
                .get_capability(cmd)
                .await
                .unwrap()
                .unwrap()
                .versions
                .len(),
            1
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
                &[version("1", "Описание review")],
            )
            .await
            .unwrap();
        store
            .create_capability(
                CapabilityKind::Skill,
                "polish",
                &[version("1", "Описание polish")],
            )
            .await
            .unwrap();
        store
            .create_capability(
                CapabilityKind::Command,
                "deploy",
                &[version("1", "Команда deploy")],
            )
            .await
            .unwrap();
        store
            .create_capability(
                CapabilityKind::Command,
                "rollback",
                &[version("1", "Команда rollback")],
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
                    model: None,
                    temperature: 0.7,
                    parent: None,
                    skills: vec![cap("review", None)],
                    commands: vec![cap("deploy", None)],
                }],
            )
            .await
            .unwrap();
        let set = store.get_agent_set(set_id).await.unwrap().unwrap();
        let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
        assert_eq!(dev.skills, vec![cap("review", None)]);
        assert_eq!(dev.commands, vec![cap("deploy", None)]);
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
    async fn agent_without_pinned_version_uses_latest_skill_version() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let skill = store
            .create_capability(
                CapabilityKind::Skill,
                "review",
                &[version("1", "Формат диффов")],
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
                    model: None,
                    temperature: 0.7,
                    parent: None,
                    skills: vec![cap("review", None)],
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
        // Новая версия каталога подхватывается без настройки агента.
        store
            .add_capability_version(skill, "2", "Прогон тестов и правки")
            .await
            .unwrap();
        let prompt = store.agent_prompt(dev).await.unwrap();
        assert!(prompt.contains("Прогон тестов и правки"));
        assert!(!prompt.contains("Формат диффов"));
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn agent_with_pinned_version_keeps_it_after_new_release() {
        let (path, file) = temp_db_path();
        let store = TraceStore::new(&path).await.unwrap();
        let skill = store
            .create_capability(
                CapabilityKind::Skill,
                "review",
                &[version("1", "Формат диффов")],
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
                    model: None,
                    temperature: 0.7,
                    parent: None,
                    skills: vec![cap("review", Some("1"))],
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
        // Выход новой версии не трогает агента с зафиксированной версией.
        store
            .add_capability_version(skill, "2", "Прогон тестов и правки")
            .await
            .unwrap();
        let prompt = store.agent_prompt(dev).await.unwrap();
        assert!(prompt.contains("Формат диффов"));
        assert!(!prompt.contains("Прогон тестов и правки"));
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }
}
