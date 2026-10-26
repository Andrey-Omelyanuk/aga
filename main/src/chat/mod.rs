//! Модель чата: пользователи, чаты и нити, сообщения, артефакты, воркстейшны
//! и сессии. Разбита по доменам:
//! - `types` — типы и константы;
//! - `users` — пользователи чата;
//! - `chats` — чаты, участники, закрытие;
//! - `messages` — сообщения, нити, шаринг, артефакты;
//! - `sessions` — воркстейшны и сессии;
//! - `rows` — мапперы строк SQL → типы;
//! - `commands` — команды чата (`#invite` и т.п.) и `@упоминания`.
//!
//! `impl ChatStore` разнесён по файлам: Rust позволяет несколько impl-блоков
//! одного типа в одном модуле, поэтому методы живут рядом со своим доменом,
//! а тип и схема БД — здесь.

use sqlx::{Row, SqlitePool};

mod chats;
mod commands;
mod messages;
mod rows;
mod sessions;
mod types;
mod users;

pub use commands::*;
pub use types::*;

#[cfg(test)]
mod tests;

pub struct ChatStore {
    pool: SqlitePool,
}

impl Clone for ChatStore {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
        }
    }
}

impl ChatStore {
    pub async fn new(db_path: &str) -> Result<Self, sqlx::Error> {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let options = SqliteConnectOptions::from_str(db_path)?.create_if_missing(true);
        let pool = SqlitePoolOptions::new().connect_with(options).await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS chat_users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'human',
                is_super_user INTEGER NOT NULL DEFAULT 0,
                sso_subject TEXT,
                role TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS workstations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                state TEXT NOT NULL DEFAULT 'creating',
                secret TEXT,
                current_session_id INTEGER,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS chats (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                root_id INTEGER NOT NULL,
                parent_id INTEGER,
                level INTEGER NOT NULL DEFAULT 0,
                title TEXT,
                start_message_id INTEGER,
                created_by_id INTEGER NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                state TEXT NOT NULL DEFAULT 'OPEN',
                FOREIGN KEY (created_by_id) REFERENCES chat_users(id)
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Сессии воркстейшнов: корневой чат воркстейшна + привязка к проекту
        // напрямую. Сессионные поля (result_id, workstation_id,
        // continues_session_id) живут здесь, а не в chats. Проекты объявляются
        // в trace.rs (TraceStore) — БД одна, и trace_store создаётся раньше
        // chat_store, поэтому FK на projects валиден.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_id INTEGER NOT NULL UNIQUE REFERENCES chats(id),
                project_id INTEGER NOT NULL,
                workstation_id INTEGER NOT NULL REFERENCES workstations(id),
                owner_id INTEGER NOT NULL REFERENCES chat_users(id),
                result_id INTEGER,
                continues_session_id INTEGER,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                closed_at DATETIME
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS chat_participants (
                chat_id INTEGER NOT NULL,
                chat_user_id INTEGER NOT NULL,
                PRIMARY KEY (chat_id, chat_user_id),
                FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE CASCADE,
                FOREIGN KEY (chat_user_id) REFERENCES chat_users(id)
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_id INTEGER NOT NULL,
                parent_id INTEGER,
                author_id INTEGER NOT NULL,
                shared_by_id INTEGER,
                share_of_id INTEGER,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                last_message_id INTEGER,
                title TEXT,
                thread_of_id INTEGER,
                body TEXT NOT NULL,
                hidden TEXT NOT NULL DEFAULT '',
                origin TEXT NOT NULL DEFAULT 'user',
                FOREIGN KEY (chat_id) REFERENCES chats(id)
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS artifacts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                message_id INTEGER NOT NULL,
                kind TEXT NOT NULL,
                title TEXT,
                content TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (message_id) REFERENCES messages(id)
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_chats_root ON chats(root_id)")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_messages_chat ON messages(chat_id)")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_messages_parent ON messages(parent_id)")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_artifacts_message ON artifacts(message_id)")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await?;

        let store = Self { pool };

        // Миграции существующих БД: новые колонки появляются только на CREATE,
        // поэтому досоздаём их вручную, если таблицы уже были.
        store
            .ensure_column("workstations", "secret", "secret TEXT")
            .await?;
        store
            .ensure_column(
                "workstations",
                "current_session_id",
                "current_session_id INTEGER",
            )
            .await?;
        store
            .ensure_column("chats", "start_message_id", "start_message_id INTEGER")
            .await?;
        store
            .ensure_column("messages", "title", "title TEXT")
            .await?;
        store
            .ensure_column("messages", "thread_of_id", "thread_of_id INTEGER")
            .await?;
        store
            .ensure_column("messages", "hidden", "hidden TEXT NOT NULL DEFAULT ''")
            .await?;
        store
            .ensure_column("messages", "origin", "origin TEXT NOT NULL DEFAULT 'user'")
            .await?;
        store
            .ensure_column("messages", "kind", "kind TEXT NOT NULL DEFAULT 'text'")
            .await?;
        store
            .ensure_column("messages", "tool_call", "tool_call TEXT")
            .await?;

        // Миграция старых БД: сессионные поля жили в chats, теперь — в
        // sessions. Переносим существующие корневые сессии в sessions.
        store.migrate_sessions().await?;
        // Проект станции больше не хранится в workstations: он живёт в
        // sessions (станция обслуживает сессии). Удаляем мёртвую колонку
        // старых БД (после migrate_sessions, которая читает w.project_id).
        store.drop_workstation_project_id().await?;

        // Аноним-суперпользователь создаётся автоматически.
        if !store.user_exists_by_kind("anonymous").await? {
            store
                .insert_user("anonymous", "anonymous", true, None, None)
                .await?;
        }

        Ok(store)
    }

    async fn user_exists_by_kind(&self, kind: &str) -> Result<bool, sqlx::Error> {
        let row = sqlx::query("SELECT COUNT(*) as c FROM chat_users WHERE kind = ?")
            .bind(kind)
            .fetch_one(&self.pool)
            .await?;
        let c: i64 = row.get("c");
        Ok(c > 0)
    }

    /// Есть ли колонка в таблице (по PRAGMA table_info).
    async fn table_has_column(&self, table: &str, column: &str) -> Result<bool, sqlx::Error> {
        let rows = sqlx::query(&format!("PRAGMA table_info({table})"))
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().any(|r| r.get::<String, _>("name") == column))
    }

    /// Добавить колонку в существующую таблицу, если её ещё нет (миграция
    /// старых БД, где CREATE TABLE IF NOT EXISTS её не создал).
    async fn ensure_column(&self, table: &str, column: &str, ddl: &str) -> Result<(), sqlx::Error> {
        if !self.table_has_column(table, column).await? {
            sqlx::query(&format!("ALTER TABLE {table} ADD COLUMN {ddl}"))
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    /// Миграция старых БД: `workstations.project_id` убран — станция больше не
    /// «назначена на проект», проект станции определяется по её сессии
    /// (`sessions.project_id`). Колонка удаляется после `migrate_sessions`,
    /// которая перенесла привязки в `sessions`.
    async fn drop_workstation_project_id(&self) -> Result<(), sqlx::Error> {
        if self.table_has_column("workstations", "project_id").await? {
            sqlx::query("ALTER TABLE workstations DROP COLUMN project_id")
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    /// Миграция старых БД: сессионные поля жили в `chats` (`result_id`,
    /// `workstation_id`, `continues_session_id`), теперь они — в `sessions`.
    /// Для каждого корневого чата (`root_id = id`) с привязкой к воркстейшну
    /// создаём запись в `sessions` и проставляем `workstations.current_session_id`.
    /// Идемпотентно: вставляем только если для `chat_id` в `sessions` ещё нет
    /// записи.
    async fn migrate_sessions(&self) -> Result<(), sqlx::Error> {
        // Старые БД уже имеют колонку workstation_id в chats и project_id в
        // workstations; новые БД их не создают, а старые — теряют после
        // миграции. Нет любой из нужных колонок — мигрировать нечего.
        if !self.table_has_column("chats", "workstation_id").await?
            || !self.table_has_column("workstations", "project_id").await?
        {
            return Ok(());
        }

        sqlx::query(
            "INSERT INTO sessions (chat_id, project_id, workstation_id, owner_id, result_id, continues_session_id) \
             SELECT c.id, w.project_id, c.workstation_id, c.created_by_id, c.result_id, c.continues_session_id \
             FROM chats c JOIN workstations w ON w.id = c.workstation_id \
             WHERE c.root_id = c.id AND c.workstation_id IS NOT NULL \
               AND NOT EXISTS (SELECT 1 FROM sessions s WHERE s.chat_id = c.id)",
        )
        .execute(&self.pool)
        .await?;

        // Проставляем current_session_id для открытых сессий старых БД.
        sqlx::query(
            "UPDATE workstations SET current_session_id = (\
               SELECT s.chat_id FROM sessions s WHERE s.workstation_id = workstations.id \
                 AND s.closed_at IS NULL \
                 AND EXISTS (SELECT 1 FROM chats c WHERE c.id = s.chat_id AND c.state = 'OPEN') \
               ORDER BY s.id DESC LIMIT 1) \
             WHERE EXISTS (SELECT 1 FROM sessions s WHERE s.workstation_id = workstations.id)",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Полностью очистить таблицы модели чата и сбросить автоинкрементные
    /// счётчики (детерминированные ID после пересоздания). Используется
    /// тестовым набором (`aga seed`).
    pub(crate) async fn clear_all(&self) -> Result<(), sqlx::Error> {
        // Дети раньше родителей (внешние ключи без каскада в части таблиц).
        for table in [
            "artifacts",
            "messages",
            "chat_participants",
            "sessions",
            "chats",
            "workstations",
            "chat_users",
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
