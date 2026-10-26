use super::rows::*;
use super::*;

impl ChatStore {
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
            "SELECT c.id, c.root_id, c.parent_id, c.level, c.title, c.start_message_id, c.created_by_id, c.created_at, c.updated_at, c.state, s.workstation_id, COALESCE(s.project_id, 0) AS project_id FROM chats c LEFT JOIN sessions s ON s.chat_id = c.id WHERE c.id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| chat_from_row(&r)))
    }

    /// Список чатов для пользователя. Все участники видят все сессии
    /// (персональной видимости нет), поэтому фильтра нет. В списке — только
    /// корневые чаты: нити (дочерние чаты) живут внутри родителя, свёрнутые
    /// у сообщения (см. историю message-threads).
    pub async fn list_chats_for_user(&self, _user_id: i64) -> Result<Vec<Chat>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT c.id, c.root_id, c.parent_id, c.level, c.title, c.start_message_id, c.created_by_id, c.created_at, c.updated_at, c.state, s.workstation_id, COALESCE(s.project_id, 0) AS project_id FROM chats c LEFT JOIN sessions s ON s.chat_id = c.id WHERE c.root_id = c.id ORDER BY c.updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(chat_from_row).collect())
    }

    /// Дочерние чаты-нити, начатые в этом чате (прямые наследники).
    pub async fn list_threads(&self, parent_chat_id: i64) -> Result<Vec<Chat>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT c.id, c.root_id, c.parent_id, c.level, c.title, c.start_message_id, c.created_by_id, c.created_at, c.updated_at, c.state, s.workstation_id, COALESCE(s.project_id, 0) AS project_id FROM chats c LEFT JOIN sessions s ON s.chat_id = c.id WHERE c.parent_id = ? ORDER BY c.id",
        )
        .bind(parent_chat_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(chat_from_row).collect())
    }

    /// Создать чат: общий чат (`parent_id = None`) или нить (`parent_id = Some`).
    /// Сессии здесь не создаются: корневой чат-сессию создаёт
    /// `open_workstation_session` (привязка к станции и проекту живёт в
    /// `sessions`).
    pub async fn create_chat(
        &self,
        parent_id: Option<i64>,
        title: Option<&str>,
        created_by_id: i64,
    ) -> Result<Chat, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let (root_id, level) = match parent_id {
            Some(pid) => {
                let parent = self.get_chat(pid).await?.ok_or(sqlx::Error::RowNotFound)?;
                if parent.state != "OPEN" {
                    return Err(sqlx::Error::RowNotFound);
                }
                if parent.level + 1 > max_chat_level() {
                    return Err(sqlx::Error::RowNotFound);
                }
                (parent.root_id, parent.level + 1)
            }
            None => (0, 0),
        };

        let result = sqlx::query(
            "INSERT INTO chats (root_id, parent_id, level, title, created_by_id) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(root_id)
        .bind(parent_id)
        .bind(level)
        .bind(title)
        .bind(created_by_id)
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

    pub(super) async fn add_participant_tx(
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

    pub async fn close_chat(&self, chat_id: i64) -> Result<bool, sqlx::Error> {
        let chat = self.get_chat(chat_id).await?;
        let Some(chat) = chat else { return Ok(false) };
        if chat.root_id == chat.id {
            sqlx::query("UPDATE chats SET state = 'CLOSED' WHERE root_id = ?")
                .bind(chat.id)
                .execute(&self.pool)
                .await?;
            self.close_session_if_any(chat.id).await?;
        } else {
            sqlx::query("UPDATE chats SET state = 'CLOSED' WHERE id = ?")
                .bind(chat_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(true)
    }

    /// Отметить сессию закрытой (если корневой чат — сессия) и освободить
    /// воркстейшн. Вызывается при закрытии корневого чата.
    async fn close_session_if_any(&self, chat_id: i64) -> Result<(), sqlx::Error> {
        let row = sqlx::query(
            "SELECT workstation_id FROM sessions WHERE chat_id = ? AND closed_at IS NULL",
        )
        .bind(chat_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else { return Ok(()) };
        let workstation_id: i64 = row.get("workstation_id");
        sqlx::query("UPDATE sessions SET closed_at = CURRENT_TIMESTAMP WHERE chat_id = ?")
            .bind(chat_id)
            .execute(&self.pool)
            .await?;
        sqlx::query(
            "UPDATE workstations SET current_session_id = NULL WHERE id = ? AND current_session_id = ?",
        )
        .bind(workstation_id)
        .bind(chat_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
