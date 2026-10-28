use super::rows::*;
use super::*;

impl ChatStore {
    pub async fn send_message(
        &self,
        chat_id: i64,
        author_id: i64,
        body: &str,
        hidden: &str,
        parent_id: Option<i64>,
        last_message_id: Option<i64>,
    ) -> Result<Option<Message>, sqlx::Error> {
        self.send_message_with_origin(
            chat_id,
            author_id,
            body,
            hidden,
            parent_id,
            last_message_id,
            "user",
        )
        .await
    }

    /// Сообщение с указанным источником (`origin`): чат его не интерпретирует —
    /// пишет как есть. HTTP-путь (набор человеком) идёт с 'user'; ответ
    /// агент-процесса пишется с 'agent' (см. `runtime.rs`), чтобы подписчик не
    /// реагировал на нечеловеческие записи.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_message_with_origin(
        &self,
        chat_id: i64,
        author_id: i64,
        body: &str,
        hidden: &str,
        parent_id: Option<i64>,
        last_message_id: Option<i64>,
        origin: &str,
    ) -> Result<Option<Message>, sqlx::Error> {
        self.insert_message(
            chat_id,
            author_id,
            body,
            hidden,
            parent_id,
            last_message_id,
            origin,
            KIND_TEXT,
            None,
        )
        .await
    }

    /// Шаг работы с инструментом: тело — команда, скрытая часть — вывод,
    /// `tool_call` — вызов JSON-ом (по нему контекст агента восстанавливает
    /// его прошлые вызовы).
    pub async fn send_step(
        &self,
        chat_id: i64,
        author_id: i64,
        body: &str,
        hidden: &str,
        tool_call: &str,
        origin: &str,
    ) -> Result<Option<Message>, sqlx::Error> {
        self.insert_message(
            chat_id,
            author_id,
            body,
            hidden,
            None,
            None,
            origin,
            KIND_STEP,
            Some(tool_call),
        )
        .await
    }

    /// Нить заполнена (`max_thread_messages()`) — новые сообщения не принимаются.
    /// Корневые чаты не ограничены.
    async fn thread_full(&self, chat: &Chat) -> Result<bool, sqlx::Error> {
        if chat.level == 0 {
            return Ok(false);
        }
        let row = sqlx::query("SELECT COUNT(*) AS c FROM messages WHERE chat_id = ?")
            .bind(chat.id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get::<i64, _>("c") >= max_thread_messages())
    }

    #[allow(clippy::too_many_arguments)]
    async fn insert_message(
        &self,
        chat_id: i64,
        author_id: i64,
        body: &str,
        hidden: &str,
        parent_id: Option<i64>,
        last_message_id: Option<i64>,
        origin: &str,
        kind: &str,
        tool_call: Option<&str>,
    ) -> Result<Option<Message>, sqlx::Error> {
        let chat = self.get_chat(chat_id).await?;
        let Some(chat) = chat else { return Ok(None) };
        if chat.state != "OPEN" || self.thread_full(&chat).await? {
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
            "INSERT INTO messages (chat_id, parent_id, author_id, last_message_id, body, hidden, origin, kind, tool_call) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(chat_id)
        .bind(parent_id)
        .bind(author_id)
        .bind(current_last)
        .bind(body)
        .bind(hidden)
        .bind(origin)
        .bind(kind)
        .bind(tool_call)
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
            "SELECT id, chat_id, parent_id, author_id, shared_by_id, share_of_id, created_at, last_message_id, title, thread_of_id, body, hidden, origin, kind, tool_call FROM messages WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| message_from_row(&r)))
    }

    /// Наибольший id сообщения во всех чатах (0 — сообщений нет). Агент-рантайм
    /// берёт его отметкой старта: всё, что новее, он ещё не обработал.
    pub async fn max_message_id(&self) -> Result<i64, sqlx::Error> {
        let row = sqlx::query("SELECT COALESCE(MAX(id), 0) AS id FROM messages")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get("id"))
    }

    /// id сообщений новее `after` по возрастанию (не больше `limit`): догрузка
    /// событий, пропущенных агент-рантаймом, пока он был отключён от Centrifugo.
    pub async fn message_ids_after(&self, after: i64, limit: i64) -> Result<Vec<i64>, sqlx::Error> {
        let rows = sqlx::query("SELECT id FROM messages WHERE id > ? ORDER BY id LIMIT ?")
            .bind(after)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|r| r.get("id")).collect())
    }

    pub async fn list_messages(&self, chat_id: i64) -> Result<Vec<Message>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, chat_id, parent_id, author_id, shared_by_id, share_of_id, created_at, last_message_id, title, thread_of_id, body, hidden, origin, kind, tool_call FROM messages WHERE chat_id = ? ORDER BY created_at, id",
        )
        .bind(chat_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(message_from_row).collect())
    }

    /// Начать нить от сообщения: дочерний чат с привязкой к сообщению
    /// (`start_message_id`) и первым сообщением, несущим заголовок нити.
    /// Заголовок у самого чата-нити не хранится — у нити он один, на первом
    /// сообщении. От одного сообщения можно начать сколько угодно нитей.
    /// Первое сообщение нити отвечает на сообщение-источник (`parent_id`).
    pub async fn start_thread(
        &self,
        parent_chat_id: i64,
        message_id: i64,
        title: &str,
        body: &str,
        hidden: &str,
        author_id: i64,
    ) -> Result<Option<(Chat, Message)>, sqlx::Error> {
        let parent = self.get_chat(parent_chat_id).await?;
        let Some(parent) = parent else {
            return Ok(None);
        };
        if parent.state != "OPEN" || parent.level + 1 > max_chat_level() {
            return Ok(None);
        }
        let origin = self.get_message(message_id).await?;
        let Some(origin) = origin else {
            return Ok(None);
        };
        if origin.chat_id != parent_chat_id {
            return Ok(None);
        }

        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            "INSERT INTO chats (root_id, parent_id, level, title, start_message_id, created_by_id) VALUES (?, ?, ?, NULL, ?, ?)",
        )
        .bind(parent.root_id)
        .bind(parent_chat_id)
        .bind(parent.level + 1)
        .bind(message_id)
        .bind(author_id)
        .execute(&mut *tx)
        .await?;
        let thread_id = result.last_insert_rowid();
        self.add_participant_tx(&mut tx, thread_id, author_id)
            .await?;

        let first = sqlx::query(
            "INSERT INTO messages (chat_id, parent_id, author_id, title, body, hidden) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(thread_id)
        .bind(message_id)
        .bind(author_id)
        .bind(title)
        .bind(body)
        .bind(hidden)
        .execute(&mut *tx)
        .await?;
        let first_id = first.last_insert_rowid();

        tx.commit().await?;
        let thread = self
            .get_chat(thread_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        let first_message = self
            .get_message(first_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        Ok(Some((thread, first_message)))
    }

    /// Отправить сообщение нити в родительский чат: оригинал остаётся в нити,
    /// а в ленте родителя появляется копия со ссылкой (`thread_of_id`) на
    /// сообщение, от которого нить началась. Автор копии — автор оригинала.
    pub async fn post_to_parent(
        &self,
        thread_message_id: i64,
    ) -> Result<Option<Message>, sqlx::Error> {
        let original = self.get_message(thread_message_id).await?;
        let Some(original) = original else {
            return Ok(None);
        };
        let thread = self.get_chat(original.chat_id).await?;
        let Some(thread) = thread else {
            return Ok(None);
        };
        let Some(parent_id) = thread.parent_id else {
            return Ok(None);
        };
        let Some(origin_id) = thread.start_message_id else {
            return Ok(None);
        };
        let parent = self.get_chat(parent_id).await?;
        let Some(parent) = parent else {
            return Ok(None);
        };
        if parent.state != "OPEN" || self.thread_full(&parent).await? {
            return Ok(None);
        }

        let result = sqlx::query(
            "INSERT INTO messages (chat_id, parent_id, author_id, title, thread_of_id, body, hidden, origin) VALUES (?, NULL, ?, ?, ?, ?, ?, ?)",
        )
        .bind(parent_id)
        .bind(original.author_id)
        .bind(&original.title)
        .bind(origin_id)
        .bind(&original.body)
        .bind(&original.hidden)
        .bind(&original.origin)
        .execute(&self.pool)
        .await?;
        let new_id = result.last_insert_rowid();

        sqlx::query("UPDATE chats SET updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(parent_id)
            .execute(&self.pool)
            .await?;

        self.get_message(new_id).await
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
            "INSERT INTO messages (chat_id, parent_id, author_id, shared_by_id, share_of_id, body, hidden, origin) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(chat_id)
        .bind(original.parent_id)
        .bind(original.author_id)
        .bind(shared_by_id)
        .bind(share_of)
        .bind(&original.body)
        .bind(&original.hidden)
        .bind(&original.origin)
        .execute(&self.pool)
        .await?;
        let new_id = result.last_insert_rowid();

        sqlx::query("UPDATE chats SET updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(chat_id)
            .execute(&self.pool)
            .await?;

        self.get_message(new_id).await
    }

    /// Закрыть чат. Корневой чат каскадно закрывает всё дерево. Если корневой
    /// чат — сессия воркстейшна, дополнительно освобождаем воркстейшн
    /// (`current_session_id = NULL`) и отмечаем сессию закрытой (`closed_at`).
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
}
