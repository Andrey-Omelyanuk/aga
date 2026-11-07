//! Хранилище ядра (`TraceStore`): трассировка задач и всё, что живёт рядом
//! в той же БД. Разбито по доменам:
//! - `types` — публичные типы (задачи, трасса, проекты, наборы агентов,
//!   каталог способностей, подключения к LLM, MCP-серверы);
//! - `migrations` — миграции старых БД (`migrate_*`);
//! - `tasks` — задачи, трасса, токены LLM, прерванные задачи;
//! - `human` — human-запросы (`ask_human`);
//! - `projects` — проекты (git-URL);
//! - `agent_sets` — наборы агентов, дерево, привязка к проектам, промпт и
//!   LLM-конфиг агента, привязки к пользователям чата;
//! - `capabilities` — каталог способностей (скиллы и сокращения) и история;
//! - `llm` — подключения к LLM и дефолтная LLM;
//! - `mcp` — каталог MCP-серверов.
//!
//! `impl TraceStore` разнесён по файлам (как в `chat/`): методы живут рядом
//! со своим доменом, а тип, схема БД и `clear_all` — здесь.

use sqlx::SqlitePool;

mod agent_sets;
mod capabilities;
mod human;
mod llm;
mod mcp;
mod migrations;
mod projects;
mod tasks;
mod types;

use migrations::*;
pub use types::*;

#[cfg(test)]
mod tests;

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
                chat_id INTEGER,
                question_message_id INTEGER,
                agent_name TEXT NOT NULL DEFAULT '',
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

        // === Каталог способностей (скиллы и сокращения) ===
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
                is_default INTEGER NOT NULL DEFAULT 0,
                native_tools INTEGER NOT NULL DEFAULT 1
            )
            "#,
        )
        .execute(&pool)
        .await?;
        // Миграция старых БД: у подключения появились модель и флаг дефолта.
        migrate_llm_connection_columns(&pool).await?;
        migrate_task_usage_columns(&pool).await?;
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
                listen_user_id INTEGER,
                UNIQUE (set_id, name),
                FOREIGN KEY (set_id) REFERENCES agent_sets(id) ON DELETE CASCADE,
                FOREIGN KEY (parent_id) REFERENCES agents(id) ON DELETE CASCADE,
                FOREIGN KEY (llm_id) REFERENCES llm_connections(id) ON DELETE SET NULL
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Каталог MCP-серверов и их привязка к агентам (как скиллы: по id,
        // переименование не рвёт ссылку, удаление отвязывает каскадом).
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS mcp_servers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                transport TEXT NOT NULL DEFAULT 'http',
                url TEXT NOT NULL DEFAULT '',
                command TEXT NOT NULL DEFAULT '',
                api_key TEXT
            )
            "#,
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS agent_mcp_servers (
                agent_id INTEGER NOT NULL,
                server_id INTEGER NOT NULL,
                PRIMARY KEY (agent_id, server_id),
                FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE,
                FOREIGN KEY (server_id) REFERENCES mcp_servers(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Миграция старых БД: у агента появилось подключение к LLM (llm_id).
        migrate_agents_llm_column(&pool).await?;
        // Миграция старых БД: привязка агента к пользователю чата
        // (listen_user_id — слушает его и отвечает от его имени).
        migrate_agents_listen_user_column(&pool).await?;
        // Миграция старых БД: human-запрос живёт в чате — привязка к сообщению
        // вопроса (ответ — «ответ на это сообщение») и имени агента.
        migrate_human_request_chat_columns(&pool).await?;
        // Миграция старых БД: каталог команд (`kind='command'`) удалён —
        // записи вычищаются, история и привязки к агентам уходят каскадом.
        // После создания agents — иначе каскад по agent_capabilities падает
        // на несуществующей таблице (FK агента ссылается на agents).
        migrate_remove_command_capabilities(&pool).await?;

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
            "agent_mcp_servers",
            "capability_history",
            "agents",
            "agent_sets",
            "capabilities",
            "projects",
            "llm_connections",
            "mcp_servers",
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
