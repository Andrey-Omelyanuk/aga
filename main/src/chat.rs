use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

/// Максимальная глубина дерева чатов.
pub const MAX_CHAT_LEVEL: i32 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatUser {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub is_super_user: bool,
    pub sso_subject: Option<String>,
    pub role: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    pub id: i64,
    pub root_id: i64,
    pub parent_id: Option<i64>,
    pub level: i32,
    pub title: Option<String>,
    pub created_by_id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub state: String,
    pub result_id: Option<i64>,
    pub workstation_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ChatParticipant {
    pub chat_id: i64,
    pub chat_user_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub chat_id: i64,
    pub parent_id: Option<i64>,
    pub author_id: i64,
    pub shared_by_id: Option<i64>,
    pub share_of_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub last_message_id: Option<i64>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: i64,
    pub message_id: i64,
    pub kind: String,
    pub title: Option<String>,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workstation {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub state: String,
    /// Имя k8s-Secret, который при подъёме монтируется в ws (секреты для
    /// сторонних CLI). Живёт в кластере, в БД храним только имя.
    pub secret: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Прерванная (незакрытая) сессия на упавшем воркстейшне — кандидат на
/// восстановление при открытии сессии на свободном ws того же проекта.
#[derive(Debug, Clone)]
pub struct InterruptedSession {
    pub session_id: i64,
}

/// Команда, распознанная в теле сообщения. Команды — это обычные сообщения,
/// на которые есть дополнительная реакция.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatCommand {
    Invite(String),
    Kick(String),
    Start(String),
    End,
}

/// Ошибки открытия/закрытия сессии воркстейшна.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("воркстейшн не найден")]
    NotFound,
    #[error("воркстейшн не готов")]
    WorkstationNotReady,
    #[error("на воркстейшне уже открыта сессия")]
    WorkstationBusy,
    #[error("закрыть сессию может только её владелец")]
    Forbidden,
    #[error("база данных: {0}")]
    Db(#[from] sqlx::Error),
}

fn parse_dt(v: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(v)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

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
                project_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                state TEXT NOT NULL DEFAULT 'creating',
                secret TEXT,
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
                created_by_id INTEGER NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                state TEXT NOT NULL DEFAULT 'OPEN',
                result_id INTEGER,
                workstation_id INTEGER,
                continues_session_id INTEGER,
                FOREIGN KEY (created_by_id) REFERENCES chat_users(id),
                FOREIGN KEY (workstation_id) REFERENCES workstations(id)
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
                body TEXT NOT NULL,
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
                "chats",
                "continues_session_id",
                "continues_session_id INTEGER",
            )
            .await?;

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

    /// Добавить колонку в существующую таблицу, если её ещё нет (миграция
    /// старых БД, где CREATE TABLE IF NOT EXISTS её не создал).
    async fn ensure_column(&self, table: &str, column: &str, ddl: &str) -> Result<(), sqlx::Error> {
        let rows = sqlx::query(&format!("PRAGMA table_info({table})"))
            .fetch_all(&self.pool)
            .await?;
        let exists = rows.iter().any(|r| {
            let name: String = r.get("name");
            name == column
        });
        if !exists {
            sqlx::query(&format!("ALTER TABLE {table} ADD COLUMN {ddl}"))
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    pub async fn insert_user(
        &self,
        name: &str,
        kind: &str,
        is_super_user: bool,
        sso_subject: Option<&str>,
        role: Option<&str>,
    ) -> Result<i64, sqlx::Error> {
        let result = sqlx::query(
            "INSERT INTO chat_users (name, kind, is_super_user, sso_subject, role) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(name)
        .bind(kind)
        .bind(is_super_user as i32)
        .bind(sso_subject)
        .bind(role)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Получить (создать при отсутствии) учётку агента для роли.
    pub async fn ensure_agent_user(&self, role: &str) -> Result<i64, sqlx::Error> {
        if let Some(id) = self.find_user_id_by_role(role).await? {
            return Ok(id);
        }
        self.insert_user(&format!("Agent.{role}"), "agent", false, None, Some(role))
            .await
    }

    async fn find_user_id_by_role(&self, role: &str) -> Result<Option<i64>, sqlx::Error> {
        let row = sqlx::query("SELECT id FROM chat_users WHERE kind = 'agent' AND role = ?")
            .bind(role)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get("id")))
    }

    pub async fn get_user(&self, id: i64) -> Result<Option<ChatUser>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, name, kind, is_super_user, sso_subject, role, created_at FROM chat_users WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| user_from_row(&r)))
    }

    pub async fn find_user_by_name(&self, name: &str) -> Result<Option<ChatUser>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, name, kind, is_super_user, sso_subject, role, created_at FROM chat_users WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| user_from_row(&r)))
    }

    pub async fn find_user_by_sso(&self, subject: &str) -> Result<Option<ChatUser>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, name, kind, is_super_user, sso_subject, role, created_at FROM chat_users WHERE sso_subject = ?",
        )
        .bind(subject)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| user_from_row(&r)))
    }

    pub async fn list_users(&self) -> Result<Vec<ChatUser>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, name, kind, is_super_user, sso_subject, role, created_at FROM chat_users ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(user_from_row).collect())
    }

    /// id аноним-суперпользователя.
    pub async fn anonymous_id(&self) -> Result<i64, sqlx::Error> {
        let row =
            sqlx::query("SELECT id FROM chat_users WHERE kind = 'anonymous' ORDER BY id LIMIT 1")
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.get("id")).unwrap_or(1))
    }

    pub async fn is_super_user(&self, id: i64) -> Result<bool, sqlx::Error> {
        let row = sqlx::query("SELECT is_super_user FROM chat_users WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row
            .map(|r| r.get::<i32, _>("is_super_user") != 0)
            .unwrap_or(false))
    }

    /// Обновить флаг суперпользователя (роль могла измениться в Keycloak).
    pub async fn set_super_user(&self, id: i64, is_super: bool) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE chat_users SET is_super_user = ? WHERE id = ?")
            .bind(is_super as i32)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Обновить отображаемое имя человека (логин из Keycloak).
    pub async fn update_user_name(&self, id: i64, name: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE chat_users SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn is_participant(&self, chat_id: i64, user_id: i64) -> Result<bool, sqlx::Error> {
        let row = sqlx::query(
            "SELECT chat_id FROM chat_participants WHERE chat_id = ? AND chat_user_id = ?",
        )
        .bind(chat_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    pub async fn is_owner(&self, chat_id: i64, user_id: i64) -> Result<bool, sqlx::Error> {
        let row = sqlx::query("SELECT created_by_id FROM chats WHERE id = ?")
            .bind(chat_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row
            .map(|r| r.get::<i64, _>("created_by_id") == user_id)
            .unwrap_or(false))
    }

    pub async fn get_chat(&self, id: i64) -> Result<Option<Chat>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, root_id, parent_id, level, title, created_by_id, created_at, updated_at, state, result_id, workstation_id FROM chats WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| chat_from_row(&r)))
    }

    /// Список чатов для пользователя. Все участники видят все сессии
    /// (персональной видимости нет), поэтому фильтра нет.
    pub async fn list_chats_for_user(&self, _user_id: i64) -> Result<Vec<Chat>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, root_id, parent_id, level, title, created_by_id, created_at, updated_at, state, result_id, workstation_id FROM chats ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(chat_from_row).collect())
    }

    /// Создать чат. Если `parent_id` пуст — создаётся корневой чат (сессия)
    /// для воркстейшна. Создатель автоматически становится участником.
    pub async fn create_chat(
        &self,
        parent_id: Option<i64>,
        title: Option<&str>,
        created_by_id: i64,
        workstation_id: Option<i64>,
    ) -> Result<Chat, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let (root_id, level) = match parent_id {
            Some(pid) => {
                let parent = self.get_chat(pid).await?.ok_or(sqlx::Error::RowNotFound)?;
                if parent.state != "OPEN" {
                    return Err(sqlx::Error::RowNotFound);
                }
                if parent.level + 1 > MAX_CHAT_LEVEL {
                    return Err(sqlx::Error::RowNotFound);
                }
                (parent.root_id, parent.level + 1)
            }
            None => (0, 0),
        };

        let result = sqlx::query(
            "INSERT INTO chats (root_id, parent_id, level, title, created_by_id, workstation_id) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(root_id)
        .bind(parent_id)
        .bind(level)
        .bind(title)
        .bind(created_by_id)
        .bind(workstation_id)
        .execute(&mut *tx)
        .await?;
        let chat_id = result.last_insert_rowid();

        if parent_id.is_none() {
            // Корневой чат: root_id = id
            sqlx::query("UPDATE chats SET root_id = ? WHERE id = ?")
                .bind(chat_id)
                .bind(chat_id)
                .execute(&mut *tx)
                .await?;
        }

        // Создатель — участник.
        self.add_participant_tx(&mut tx, chat_id, created_by_id)
            .await?;

        tx.commit().await?;
        Ok(self.get_chat(chat_id).await?.unwrap())
    }

    async fn add_participant_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        chat_id: i64,
        user_id: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR IGNORE INTO chat_participants (chat_id, chat_user_id) VALUES (?, ?)",
        )
        .bind(chat_id)
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub async fn add_participant(&self, chat_id: i64, user_id: i64) -> Result<bool, sqlx::Error> {
        let chat = self.get_chat(chat_id).await?;
        let Some(chat) = chat else { return Ok(false) };
        if chat.state != "OPEN" {
            return Ok(false);
        }
        sqlx::query(
            "INSERT OR IGNORE INTO chat_participants (chat_id, chat_user_id) VALUES (?, ?)",
        )
        .bind(chat_id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(true)
    }

    /// Добавить участника, если его ещё нет.
    pub async fn ensure_participant(&self, chat_id: i64, user_id: i64) -> Result<(), sqlx::Error> {
        self.add_participant(chat_id, user_id).await?;
        Ok(())
    }

    pub async fn remove_participant(
        &self,
        chat_id: i64,
        user_id: i64,
    ) -> Result<bool, sqlx::Error> {
        let result =
            sqlx::query("DELETE FROM chat_participants WHERE chat_id = ? AND chat_user_id = ?")
                .bind(chat_id)
                .bind(user_id)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_participants(&self, chat_id: i64) -> Result<Vec<ChatUser>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT u.id, u.name, u.kind, u.is_super_user, u.sso_subject, u.role, u.created_at \
             FROM chat_participants p JOIN chat_users u ON u.id = p.chat_user_id WHERE p.chat_id = ? ORDER BY u.id",
        )
        .bind(chat_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(user_from_row).collect())
    }

    pub async fn send_message(
        &self,
        chat_id: i64,
        author_id: i64,
        body: &str,
        parent_id: Option<i64>,
        last_message_id: Option<i64>,
    ) -> Result<Option<Message>, sqlx::Error> {
        let chat = self.get_chat(chat_id).await?;
        let Some(chat) = chat else { return Ok(None) };
        if chat.state != "OPEN" {
            return Ok(None);
        }

        let last_msgs: Vec<i64> =
            sqlx::query("SELECT id FROM messages WHERE chat_id = ? ORDER BY id DESC LIMIT 1")
                .bind(chat_id)
                .fetch_all(&self.pool)
                .await?
                .iter()
                .map(|r| r.get("id"))
                .collect();
        let current_last = last_msgs.first().copied().or(last_message_id);

        let result = sqlx::query(
            "INSERT INTO messages (chat_id, parent_id, author_id, last_message_id, body) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(chat_id)
        .bind(parent_id)
        .bind(author_id)
        .bind(current_last)
        .bind(body)
        .execute(&self.pool)
        .await?;
        let msg_id = result.last_insert_rowid();

        sqlx::query("UPDATE chats SET updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(chat_id)
            .execute(&self.pool)
            .await?;

        self.get_message(msg_id).await
    }

    pub async fn get_message(&self, id: i64) -> Result<Option<Message>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, chat_id, parent_id, author_id, shared_by_id, share_of_id, created_at, last_message_id, body FROM messages WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| message_from_row(&r)))
    }

    pub async fn list_messages(&self, chat_id: i64) -> Result<Vec<Message>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, chat_id, parent_id, author_id, shared_by_id, share_of_id, created_at, last_message_id, body FROM messages WHERE chat_id = ? ORDER BY created_at, id",
        )
        .bind(chat_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(message_from_row).collect())
    }

    /// Зашарить сообщение в целевой чат. Копия несёт тело целиком;
    /// `share_of_id` всегда указывает на изначальный оригинал.
    pub async fn share_message(
        &self,
        chat_id: i64,
        message_id: i64,
        shared_by_id: i64,
    ) -> Result<Option<Message>, sqlx::Error> {
        let original = self.get_message(message_id).await?;
        let Some(original) = original else {
            return Ok(None);
        };

        let target = self.get_chat(chat_id).await?;
        let Some(target) = target else {
            return Ok(None);
        };
        if target.state != "OPEN" || target.id == original.chat_id {
            return Ok(None);
        }

        let share_of = original.share_of_id.unwrap_or(original.id);
        let result = sqlx::query(
            "INSERT INTO messages (chat_id, parent_id, author_id, shared_by_id, share_of_id, body) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(chat_id)
        .bind(original.parent_id)
        .bind(original.author_id)
        .bind(shared_by_id)
        .bind(share_of)
        .bind(&original.body)
        .execute(&self.pool)
        .await?;
        let new_id = result.last_insert_rowid();

        sqlx::query("UPDATE chats SET updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(chat_id)
            .execute(&self.pool)
            .await?;

        self.get_message(new_id).await
    }

    /// Закрыть чат. Корневой чат каскадно закрывает всё дерево.
    pub async fn close_chat(&self, chat_id: i64) -> Result<bool, sqlx::Error> {
        let chat = self.get_chat(chat_id).await?;
        let Some(chat) = chat else { return Ok(false) };
        if chat.root_id == chat.id {
            sqlx::query("UPDATE chats SET state = 'CLOSED' WHERE root_id = ?")
                .bind(chat.id)
                .execute(&self.pool)
                .await?;
        } else {
            sqlx::query("UPDATE chats SET state = 'CLOSED' WHERE id = ?")
                .bind(chat_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(true)
    }

    /// Установить сообщение-итог чата. Только владелец, сообщение из чата.
    #[allow(dead_code)]
    pub async fn set_result(
        &self,
        chat_id: i64,
        message_id: i64,
        by_user_id: i64,
    ) -> Result<bool, sqlx::Error> {
        if !self.is_owner(chat_id, by_user_id).await? {
            return Ok(false);
        }
        let msg = self.get_message(message_id).await?;
        let Some(msg) = msg else { return Ok(false) };
        if msg.chat_id != chat_id {
            return Ok(false);
        }
        sqlx::query("UPDATE chats SET result_id = ? WHERE id = ?")
            .bind(message_id)
            .bind(chat_id)
            .execute(&self.pool)
            .await?;
        Ok(true)
    }

    pub async fn add_artifact(
        &self,
        message_id: i64,
        kind: &str,
        title: Option<&str>,
        content: &str,
    ) -> Result<Artifact, sqlx::Error> {
        let result = sqlx::query(
            "INSERT INTO artifacts (message_id, kind, title, content) VALUES (?, ?, ?, ?)",
        )
        .bind(message_id)
        .bind(kind)
        .bind(title)
        .bind(content)
        .execute(&self.pool)
        .await?;
        let id = result.last_insert_rowid();
        let row = sqlx::query(
            "SELECT id, message_id, kind, title, content, created_at FROM artifacts WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(artifact_from_row(&row))
    }

    pub async fn list_artifacts(&self, message_id: i64) -> Result<Vec<Artifact>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, message_id, kind, title, content, created_at FROM artifacts WHERE message_id = ? ORDER BY id",
        )
        .bind(message_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(artifact_from_row).collect())
    }

    // === Workstations ===

    pub async fn create_workstation(
        &self,
        project_id: i64,
        name: &str,
        secret: Option<&str>,
    ) -> Result<Workstation, sqlx::Error> {
        let result = sqlx::query(
            "INSERT INTO workstations (project_id, name, state, secret) VALUES (?, ?, 'creating', ?)",
        )
        .bind(project_id)
        .bind(name)
        .bind(secret)
        .execute(&self.pool)
        .await?;
        let id = result.last_insert_rowid();
        self.get_workstation(id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }

    pub async fn get_workstation(&self, id: i64) -> Result<Option<Workstation>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, project_id, name, state, secret, created_at FROM workstations WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| Workstation {
            id: r.get("id"),
            project_id: r.get("project_id"),
            name: r.get("name"),
            state: r.get("state"),
            secret: r.get("secret"),
            created_at: parse_dt(&r.get::<String, _>("created_at")),
        }))
    }

    pub async fn list_workstations(&self) -> Result<Vec<Workstation>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, project_id, name, state, secret, created_at FROM workstations ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| Workstation {
                id: r.get("id"),
                project_id: r.get("project_id"),
                name: r.get("name"),
                state: r.get("state"),
                secret: r.get("secret"),
                created_at: parse_dt(&r.get::<String, _>("created_at")),
            })
            .collect())
    }

    #[allow(dead_code)]
    /// Удалить воркстейшн из списка.
    pub async fn delete_workstation(&self, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM workstations WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn set_workstation_state(&self, id: i64, state: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE workstations SET state = ? WHERE id = ?")
            .bind(state)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Отметить упавший воркстейшн как недоступный (`down`). Прогресс его
    /// сессий не теряется — восстанавливается на другой станции.
    pub async fn mark_workstation_down(&self, id: i64) -> Result<(), sqlx::Error> {
        self.set_workstation_state(id, "down").await
    }

    /// Переключить воркстейшн на другой проект. Только на свободной станции
    /// (открытой сессии быть не должно) — иначе `WorkstationBusy`. Сам ws не
    /// пересоздаётся: файлы проекта переписывает вызывающий (см. ws_ops).
    pub async fn switch_workstation_project(
        &self,
        ws_id: i64,
        new_project_id: i64,
    ) -> Result<Workstation, SessionError> {
        if self.get_workstation(ws_id).await?.is_none() {
            return Err(SessionError::NotFound);
        }
        if self.active_session_id(ws_id).await?.is_some() {
            return Err(SessionError::WorkstationBusy);
        }
        sqlx::query("UPDATE workstations SET project_id = ? WHERE id = ?")
            .bind(new_project_id)
            .bind(ws_id)
            .execute(&self.pool)
            .await?;
        self.get_workstation(ws_id)
            .await?
            .ok_or(SessionError::NotFound)
    }

    /// Отпустить воркстейшн: станция становится свободной (не привязана ни к
    /// одному проекту). Свобода кодируется `project_id = 0` — сантинел, id
    /// проектов начинаются с 1 (AUTOINCREMENT), внешних ключей на него нет.
    /// Только на свободной станции (без открытой сессии) — иначе
    /// `WorkstationBusy`.
    pub async fn release_workstation(&self, ws_id: i64) -> Result<Workstation, SessionError> {
        if self.get_workstation(ws_id).await?.is_none() {
            return Err(SessionError::NotFound);
        }
        if self.active_session_id(ws_id).await?.is_some() {
            return Err(SessionError::WorkstationBusy);
        }
        sqlx::query("UPDATE workstations SET project_id = 0 WHERE id = ?")
            .bind(ws_id)
            .execute(&self.pool)
            .await?;
        self.get_workstation(ws_id)
            .await?
            .ok_or(SessionError::NotFound)
    }

    /// Прерванная (незакрытая) сессия на упавшем воркстейшне того же проекта —
    /// кандидат на восстановление. Берём самую свежую по `updated_at`.
    pub async fn interrupted_session_for_project(
        &self,
        project_id: i64,
    ) -> Result<Option<InterruptedSession>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT c.id AS sid FROM chats c \
             JOIN workstations w ON w.id = c.workstation_id \
             WHERE c.state = 'OPEN' AND c.root_id = c.id \
               AND w.state = 'down' AND w.project_id = ? \
             ORDER BY c.updated_at DESC LIMIT 1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| InterruptedSession {
            session_id: r.get("sid"),
        }))
    }

    /// Ссылка сессии (`chat_id`) на прерванную, которую она продолжает.
    pub async fn continues_session_id(&self, chat_id: i64) -> Result<Option<i64>, sqlx::Error> {
        let row = sqlx::query("SELECT continues_session_id FROM chats WHERE id = ?")
            .bind(chat_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.and_then(|r| r.get("continues_session_id")))
    }

    async fn set_continues_session(&self, chat_id: i64, prev: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE chats SET continues_session_id = ? WHERE id = ?")
            .bind(prev)
            .bind(chat_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Ветка (`ws-<id>`), на которой работал корневой чат-сессия. Нужна для
    /// восстановления файлов прерванной сессии с упавшей станции.
    pub async fn session_branch(&self, session_id: i64) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query("SELECT workstation_id FROM chats WHERE id = ? AND root_id = id")
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row
            .and_then(|r| r.get::<Option<i64>, _>("workstation_id"))
            .map(crate::cluster::Cluster::branch_name))
    }

    /// Найти активную (открытую) сессию воркстейшна.
    pub async fn active_session_id(&self, workstation_id: i64) -> Result<Option<i64>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id FROM chats WHERE workstation_id = ? AND state = 'OPEN' AND root_id = id ORDER BY id DESC LIMIT 1",
        )
        .bind(workstation_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.get("id")))
    }

    /// Открыть сессию на воркстейшне: корневой чат, привязанный к воркстейшну.
    /// Только на готовом воркстейшне и когда открытых сессий на нём нет.
    ///
    /// Восстановление после падения — ручное: если на свободном ws открывают
    /// сессию, а на упавшем ws есть незакрытая сессия того же проекта, то эта
    /// сессия считается её продолжением (`continues_session_id`) — файлы
    /// восстанавливает вызывающий из ветки упавшей станции.
    pub async fn open_workstation_session(
        &self,
        workstation_id: i64,
        title: Option<&str>,
        created_by_id: i64,
    ) -> Result<crate::chat::Chat, SessionError> {
        let ws = self
            .get_workstation(workstation_id)
            .await?
            .ok_or(SessionError::NotFound)?;
        if ws.state != "ready" {
            return Err(SessionError::WorkstationNotReady);
        }
        if self.active_session_id(workstation_id).await?.is_some() {
            return Err(SessionError::WorkstationBusy);
        }
        let chat = self
            .create_chat(None, title, created_by_id, Some(workstation_id))
            .await?;
        if let Some(interrupted) = self.interrupted_session_for_project(ws.project_id).await? {
            self.set_continues_session(chat.id, interrupted.session_id)
                .await?;
        }
        Ok(chat)
    }

    /// Закрыть сессию воркстейшна. Только владелец сессии (или суперпользователь
    /// локального режима) — закрытие освобождает воркстейшн.
    pub async fn close_workstation_session(
        &self,
        chat_id: i64,
        by_user_id: i64,
    ) -> Result<(), SessionError> {
        self.get_chat(chat_id)
            .await?
            .ok_or(SessionError::NotFound)?;
        let is_super = self.is_super_user(by_user_id).await?;
        let is_owner = self.is_owner(chat_id, by_user_id).await?;
        if !is_super && !is_owner {
            return Err(SessionError::Forbidden);
        }
        self.close_chat(chat_id).await?;
        Ok(())
    }

    /// Воркстейшн корневого чата, к которому принадлежит чат.
    pub async fn root_workstation_id(&self, chat_id: i64) -> Result<Option<i64>, sqlx::Error> {
        let mut id = chat_id;
        let mut seen = 0;
        loop {
            let row =
                sqlx::query("SELECT root_id, parent_id, workstation_id FROM chats WHERE id = ?")
                    .bind(id)
                    .fetch_optional(&self.pool)
                    .await?;
            let Some(r) = row else { return Ok(None) };
            if seen > MAX_CHAT_LEVEL {
                return Ok(None);
            }
            seen += 1;
            let root_id: i64 = r.get("root_id");
            let parent_id: Option<i64> = r.get("parent_id");
            let workstation_id: Option<i64> = r.get("workstation_id");
            if parent_id.is_none() || root_id == id {
                return Ok(workstation_id);
            }
            id = parent_id.unwrap();
        }
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

fn user_from_row(r: &sqlx::sqlite::SqliteRow) -> ChatUser {
    ChatUser {
        id: r.get("id"),
        name: r.get("name"),
        kind: r.get("kind"),
        is_super_user: r.get::<i32, _>("is_super_user") != 0,
        sso_subject: r.get("sso_subject"),
        role: r.get("role"),
        created_at: parse_dt(&r.get::<String, _>("created_at")),
    }
}

fn chat_from_row(r: &sqlx::sqlite::SqliteRow) -> Chat {
    Chat {
        id: r.get("id"),
        root_id: r.get("root_id"),
        parent_id: r.get("parent_id"),
        level: r.get("level"),
        title: r.get("title"),
        created_by_id: r.get("created_by_id"),
        created_at: parse_dt(&r.get::<String, _>("created_at")),
        updated_at: parse_dt(&r.get::<String, _>("updated_at")),
        state: r.get("state"),
        result_id: r.get("result_id"),
        workstation_id: r.get("workstation_id"),
    }
}

fn message_from_row(r: &sqlx::sqlite::SqliteRow) -> Message {
    Message {
        id: r.get("id"),
        chat_id: r.get("chat_id"),
        parent_id: r.get("parent_id"),
        author_id: r.get("author_id"),
        shared_by_id: r.get("shared_by_id"),
        share_of_id: r.get("share_of_id"),
        created_at: parse_dt(&r.get::<String, _>("created_at")),
        last_message_id: r.get("last_message_id"),
        body: r.get("body"),
    }
}

fn artifact_from_row(r: &sqlx::sqlite::SqliteRow) -> Artifact {
    Artifact {
        id: r.get("id"),
        message_id: r.get("message_id"),
        kind: r.get("kind"),
        title: r.get("title"),
        content: r.get("content"),
        created_at: parse_dt(&r.get::<String, _>("created_at")),
    }
}

/// Распознать команду по первой строке сообщения. Не команда → None.
pub fn parse_command(body: &str) -> Option<ChatCommand> {
    let first = body.lines().next()?.trim();
    let mut parts = first.split_whitespace();
    let cmd = parts.next()?;
    match cmd {
        "#invite" => parts.next().map(|n| ChatCommand::Invite(clean_at(n))),
        "#kick" => parts.next().map(|n| ChatCommand::Kick(clean_at(n))),
        "#start" => {
            let title = first
                .split_whitespace()
                .skip(1)
                .collect::<Vec<_>>()
                .join(" ");
            if title.is_empty() {
                None
            } else {
                Some(ChatCommand::Start(title))
            }
        }
        "#end" => Some(ChatCommand::End),
        _ => None,
    }
}

fn clean_at(name: &str) -> String {
    name.trim_start_matches('@').to_string()
}

/// Найти всех агентов, упомянутых в сообщении вида `@Agent.<role>`.
pub fn mentioned_roles(body: &str) -> Vec<String> {
    let mut roles = Vec::new();
    for token in body.split_whitespace() {
        if let Some(rest) = token.strip_prefix("@Agent.") {
            let role = rest.trim_end_matches([',', '.', '!']).to_string();
            if !role.is_empty() {
                roles.push(role);
            }
        }
    }
    roles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(
            parse_command("#invite @B"),
            Some(ChatCommand::Invite("B".into()))
        );
        assert_eq!(
            parse_command("#kick @B"),
            Some(ChatCommand::Kick("B".into()))
        );
        assert_eq!(
            parse_command("#start New thread"),
            Some(ChatCommand::Start("New thread".into()))
        );
        assert_eq!(parse_command("#end"), Some(ChatCommand::End));
        assert_eq!(parse_command("hello"), None);
        assert_eq!(parse_command("\n#start X"), None);
    }

    #[test]
    fn mentions_roles() {
        assert_eq!(
            mentioned_roles("hi @Agent.docker-helper please"),
            vec!["docker-helper"]
        );
        assert!(mentioned_roles("no mention").is_empty());
        assert_eq!(mentioned_roles("@Agent.a @Agent.b!"), vec!["a", "b"]);
    }

    #[tokio::test]
    async fn chatstore_opens_db() {
        let path = std::env::temp_dir().join(format!("aga_chat_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let users = store.list_users().await.unwrap();
        assert!(!users.is_empty());
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn deleted_workstation_disappears_from_list() {
        let path =
            std::env::temp_dir().join(format!("aga_chat_ws_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let ws = store.create_workstation(1, "w1", None).await.unwrap();
        assert!(store
            .list_workstations()
            .await
            .unwrap()
            .iter()
            .any(|w| w.id == ws.id));
        assert!(store.delete_workstation(ws.id).await.unwrap());
        assert!(!store
            .list_workstations()
            .await
            .unwrap()
            .iter()
            .any(|w| w.id == ws.id));
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    async fn ready_ws(store: &ChatStore) -> i64 {
        let ws = store.create_workstation(1, "w1", None).await.unwrap();
        store.set_workstation_state(ws.id, "ready").await.unwrap();
        ws.id
    }

    #[tokio::test]
    async fn participant_sees_all_sessions() {
        let path =
            std::env::temp_dir().join(format!("aga_chat_see_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let participant = store
            .insert_user("bob", "human", false, None, None)
            .await
            .unwrap();
        let alice = store
            .insert_user("alice", "human", false, None, None)
            .await
            .unwrap();
        let carol = store
            .insert_user("carol", "human", false, None, None)
            .await
            .unwrap();
        let c1 = store
            .create_chat(None, Some("s1"), alice, None)
            .await
            .unwrap();
        let c2 = store
            .create_chat(None, Some("s2"), carol, None)
            .await
            .unwrap();
        let chats = store.list_chats_for_user(participant).await.unwrap();
        assert!(chats.iter().any(|c| c.id == c1.id));
        assert!(chats.iter().any(|c| c.id == c2.id));
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn session_binds_to_ready_workstation() {
        let path =
            std::env::temp_dir().join(format!("aga_chat_open_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let ws_id = ready_ws(&store).await;
        let user = store
            .insert_user("bob", "human", false, None, None)
            .await
            .unwrap();
        let chat = store
            .open_workstation_session(ws_id, Some("s1"), user)
            .await
            .unwrap();
        assert_eq!(chat.workstation_id, Some(ws_id));
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn workstation_not_ready_rejects_session() {
        let path =
            std::env::temp_dir().join(format!("aga_chat_nr_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let ws = store.create_workstation(1, "w1", None).await.unwrap();
        let user = store
            .insert_user("bob", "human", false, None, None)
            .await
            .unwrap();
        let err = store
            .open_workstation_session(ws.id, Some("s1"), user)
            .await
            .unwrap_err();
        assert!(matches!(err, SessionError::WorkstationNotReady));
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn workstation_holds_single_open_session() {
        let path =
            std::env::temp_dir().join(format!("aga_chat_busy_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let ws_id = ready_ws(&store).await;
        let user = store
            .insert_user("bob", "human", false, None, None)
            .await
            .unwrap();
        store
            .open_workstation_session(ws_id, Some("a"), user)
            .await
            .unwrap();
        let err = store
            .open_workstation_session(ws_id, Some("b"), user)
            .await
            .unwrap_err();
        assert!(matches!(err, SessionError::WorkstationBusy));
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn session_closed_only_by_owner() {
        let path =
            std::env::temp_dir().join(format!("aga_chat_owner_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let ws_id = ready_ws(&store).await;
        let owner = store
            .insert_user("owner", "human", false, None, None)
            .await
            .unwrap();
        let chat = store
            .open_workstation_session(ws_id, Some("s"), owner)
            .await
            .unwrap();
        store
            .close_workstation_session(chat.id, owner)
            .await
            .unwrap();
        assert!(store.active_session_id(ws_id).await.unwrap().is_none());
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn participant_cannot_close_foreign_session() {
        let path =
            std::env::temp_dir().join(format!("aga_chat_foreign_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let ws_id = ready_ws(&store).await;
        let owner = store
            .insert_user("owner", "human", false, None, None)
            .await
            .unwrap();
        let other = store
            .insert_user("other", "human", false, None, None)
            .await
            .unwrap();
        let chat = store
            .open_workstation_session(ws_id, Some("s"), owner)
            .await
            .unwrap();
        let err = store
            .close_workstation_session(chat.id, other)
            .await
            .unwrap_err();
        assert!(matches!(err, SessionError::Forbidden));
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn closed_session_frees_workstation() {
        let path =
            std::env::temp_dir().join(format!("aga_chat_free_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let ws_id = ready_ws(&store).await;
        let user = store
            .insert_user("bob", "human", false, None, None)
            .await
            .unwrap();
        let first = store
            .open_workstation_session(ws_id, Some("a"), user)
            .await
            .unwrap();
        store
            .close_workstation_session(first.id, user)
            .await
            .unwrap();
        let second = store
            .open_workstation_session(ws_id, Some("b"), user)
            .await
            .unwrap();
        assert_ne!(first.id, second.id);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn workstation_keeps_named_secret() {
        let path =
            std::env::temp_dir().join(format!("aga_chat_secret_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let ws = store
            .create_workstation(1, "w1", Some("creds"))
            .await
            .unwrap();
        assert_eq!(
            store
                .get_workstation(ws.id)
                .await
                .unwrap()
                .unwrap()
                .secret
                .as_deref(),
            Some("creds")
        );
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn switching_workstation_rejected_while_session_open() {
        let path = std::env::temp_dir().join(format!(
            "aga_chat_switchbusy_test_{}.db",
            uuid::Uuid::new_v4()
        ));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let ws_id = ready_ws(&store).await;
        let user = store
            .insert_user("bob", "human", false, None, None)
            .await
            .unwrap();
        store
            .open_workstation_session(ws_id, Some("s"), user)
            .await
            .unwrap();
        let err = store
            .switch_workstation_project(ws_id, 99)
            .await
            .unwrap_err();
        assert!(matches!(err, SessionError::WorkstationBusy));
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn switched_workstation_points_to_new_project() {
        let path =
            std::env::temp_dir().join(format!("aga_chat_switch_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let ws = store.create_workstation(1, "w1", None).await.unwrap();
        let switched = store.switch_workstation_project(ws.id, 7).await.unwrap();
        assert_eq!(switched.project_id, 7);
        assert_eq!(
            store
                .get_workstation(ws.id)
                .await
                .unwrap()
                .unwrap()
                .project_id,
            7
        );
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn released_workstation_becomes_free() {
        let path =
            std::env::temp_dir().join(format!("aga_chat_release_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let ws = store.create_workstation(1, "w1", None).await.unwrap();
        let released = store.release_workstation(ws.id).await.unwrap();
        // Свобода — project_id = 0 (сантинел; id проектов с 1).
        assert_eq!(released.project_id, 0);
        assert_eq!(
            store
                .get_workstation(ws.id)
                .await
                .unwrap()
                .unwrap()
                .project_id,
            0
        );
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn releasing_workstation_rejected_while_session_open() {
        let path = std::env::temp_dir().join(format!(
            "aga_chat_releasebusy_test_{}.db",
            uuid::Uuid::new_v4()
        ));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let ws_id = ready_ws(&store).await;
        let user = store
            .insert_user("bob", "human", false, None, None)
            .await
            .unwrap();
        store
            .open_workstation_session(ws_id, Some("s"), user)
            .await
            .unwrap();
        let err = store.release_workstation(ws_id).await.unwrap_err();
        assert!(matches!(err, SessionError::WorkstationBusy));
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn crashed_workstation_marked_down() {
        let path =
            std::env::temp_dir().join(format!("aga_chat_down_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let ws = store.create_workstation(1, "w1", None).await.unwrap();
        store.mark_workstation_down(ws.id).await.unwrap();
        assert_eq!(
            store.get_workstation(ws.id).await.unwrap().unwrap().state,
            "down"
        );
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn workstation_down_rejects_new_session() {
        let path = std::env::temp_dir().join(format!(
            "aga_chat_downsess_test_{}.db",
            uuid::Uuid::new_v4()
        ));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let ws = store.create_workstation(1, "w1", None).await.unwrap();
        store.mark_workstation_down(ws.id).await.unwrap();
        let user = store
            .insert_user("bob", "human", false, None, None)
            .await
            .unwrap();
        let err = store
            .open_workstation_session(ws.id, Some("s"), user)
            .await
            .unwrap_err();
        assert!(matches!(err, SessionError::WorkstationNotReady));
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn session_opened_on_free_ws_recovers_interrupted_session() {
        let path =
            std::env::temp_dir().join(format!("aga_chat_recover_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let user = store
            .insert_user("bob", "human", false, None, None)
            .await
            .unwrap();
        // Станция 1: проект 1, открыта сессия, станция упала — сессия прервана.
        let ws1 = store.create_workstation(1, "ws1", None).await.unwrap();
        store.set_workstation_state(ws1.id, "ready").await.unwrap();
        let interrupted = store
            .open_workstation_session(ws1.id, Some("s1"), user)
            .await
            .unwrap();
        store.mark_workstation_down(ws1.id).await.unwrap();
        // На свободной станции 2 того же проекта открывают сессию — она
        // распознаёт продолжение прерванной и восстанавливает её.
        let ws2 = store.create_workstation(1, "ws2", None).await.unwrap();
        store.set_workstation_state(ws2.id, "ready").await.unwrap();
        let recovered = store
            .open_workstation_session(ws2.id, Some("s2"), user)
            .await
            .unwrap();
        assert_eq!(
            store.continues_session_id(recovered.id).await.unwrap(),
            Some(interrupted.id)
        );
        assert_eq!(
            store.session_branch(interrupted.id).await.unwrap(),
            Some(crate::cluster::Cluster::branch_name(ws1.id))
        );
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn fresh_session_without_crash_has_no_continuation() {
        let path = std::env::temp_dir().join(format!(
            "aga_chat_norecover_test_{}.db",
            uuid::Uuid::new_v4()
        ));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        let user = store
            .insert_user("bob", "human", false, None, None)
            .await
            .unwrap();
        let ws_id = ready_ws(&store).await;
        let s = store
            .open_workstation_session(ws_id, Some("new"), user)
            .await
            .unwrap();
        assert!(store.continues_session_id(s.id).await.unwrap().is_none());
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&path);
    }
}
