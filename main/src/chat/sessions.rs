use super::rows::*;
use super::*;

impl ChatStore {
    #[allow(dead_code)]
    /// Установить сообщение-итог чата. Только владелец, сообщение из чата.
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
        sqlx::query("UPDATE sessions SET result_id = ? WHERE chat_id = ?")
            .bind(message_id)
            .bind(chat_id)
            .execute(&self.pool)
            .await?;
        Ok(true)
    }
    // === Workstations ===

    /// Создать запись о воркстейшне. Станция создаётся свободной и пустой:
    /// проект появляется только вместе с сессией (`open_workstation_session`).
    pub async fn create_workstation(
        &self,
        name: &str,
        secret: Option<&str>,
    ) -> Result<Workstation, sqlx::Error> {
        let result =
            sqlx::query("INSERT INTO workstations (name, state, secret) VALUES (?, 'creating', ?)")
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
            "SELECT w.id, w.name, COALESCE(s.project_id, 0) AS project_id, w.state, w.secret, w.current_session_id, w.created_at FROM workstations w LEFT JOIN sessions s ON s.chat_id = w.current_session_id WHERE w.id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| workstation_from_row(&r)))
    }

    pub async fn list_workstations(&self) -> Result<Vec<Workstation>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT w.id, w.name, COALESCE(s.project_id, 0) AS project_id, w.state, w.secret, w.current_session_id, w.created_at FROM workstations w LEFT JOIN sessions s ON s.chat_id = w.current_session_id ORDER BY w.id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(workstation_from_row).collect())
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

    /// Отпустить воркстейшн: станция остаётся без сессии (она и так обязана
    /// быть свободной — иначе `WorkstationBusy`), вызывающий очищает содержимое
    /// `/work/project` станции (`ws_ops::release_workspace`). Проект станции
    /// определяется активной сессией, в БД отпускать нечего — проверка и
    /// подтверждение.
    pub async fn release_workstation(&self, ws_id: i64) -> Result<Workstation, SessionError> {
        if self.get_workstation(ws_id).await?.is_none() {
            return Err(SessionError::NotFound);
        }
        if self.active_session_id(ws_id).await?.is_some() {
            return Err(SessionError::WorkstationBusy);
        }
        self.get_workstation(ws_id)
            .await?
            .ok_or(SessionError::NotFound)
    }

    /// Прерванная (незакрытая) сессия на упавшем воркстейшне того же проекта —
    /// кандидат на восстановление. Берём самую свежую по `updated_at` чата.
    pub async fn interrupted_session_for_project(
        &self,
        project_id: i64,
    ) -> Result<Option<InterruptedSession>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT s.chat_id AS sid FROM sessions s \
             JOIN workstations w ON w.id = s.workstation_id \
             JOIN chats c ON c.id = s.chat_id \
             WHERE c.state = 'OPEN' AND s.closed_at IS NULL \
               AND w.state = 'down' AND s.project_id = ? \
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
        let row = sqlx::query("SELECT continues_session_id FROM sessions WHERE chat_id = ?")
            .bind(chat_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.and_then(|r| r.get("continues_session_id")))
    }

    async fn set_continues_session(&self, chat_id: i64, prev: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE sessions SET continues_session_id = ? WHERE chat_id = ?")
            .bind(prev)
            .bind(chat_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Ветка (`ws-<id>`), на которой работала сессия. Нужна для
    /// восстановления файлов прерванной сессии с упавшей станции.
    pub async fn session_branch(&self, session_id: i64) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query("SELECT workstation_id FROM sessions WHERE chat_id = ?")
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row
            .and_then(|r| r.get::<Option<i64>, _>("workstation_id"))
            .map(crate::cluster::Cluster::branch_name))
    }

    /// Найти активную (открытую) сессию воркстейшна.
    pub async fn active_session_id(&self, workstation_id: i64) -> Result<Option<i64>, sqlx::Error> {
        // Активная сессия — та, что стоит в workstations.current_session_id и
        // ещё открыта (чат OPEN). Защищаемся от рассинхрона: если
        // current_session_id указывает на закрытый чат — не считаем активной.
        let row = sqlx::query(
            "SELECT w.current_session_id FROM workstations w \
             JOIN sessions s ON s.chat_id = w.current_session_id \
             JOIN chats c ON c.id = s.chat_id \
             WHERE w.id = ? AND c.state = 'OPEN' AND s.closed_at IS NULL \
             ORDER BY s.id DESC LIMIT 1",
        )
        .bind(workstation_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|r| r.get::<Option<i64>, _>("current_session_id")))
    }

    /// Открыть сессию на воркстейшне: корневой чат + запись сессии с проектом.
    /// Станция обслуживает сессии: проект задаётся здесь, а не хранится на
    /// станции. Только на готовом воркстейшне и когда открытых сессий на нём
    /// нет. Код проекта в `/work/project` разворачивает вызывающий (см.
    /// `ws_ops::replace_project` в server.rs).
    ///
    /// Восстановление после падения — ручное: если на свободном ws открывают
    /// сессию, а на упавшем ws есть незакрытая сессия того же проекта, то эта
    /// сессия считается её продолжением (`continues_session_id`) — файлы
    /// восстанавливает вызывающий из ветки упавшей станции.
    pub async fn open_workstation_session(
        &self,
        workstation_id: i64,
        project_id: i64,
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

        let mut tx = self.pool.begin().await?;

        // Корневой чат без workstation_id в chats.
        let result = sqlx::query(
            "INSERT INTO chats (root_id, parent_id, level, title, created_by_id) VALUES (0, NULL, 0, ?, ?)",
        )
        .bind(title)
        .bind(created_by_id)
        .execute(&mut *tx)
        .await?;
        let chat_id = result.last_insert_rowid();
        sqlx::query("UPDATE chats SET root_id = ? WHERE id = ?")
            .bind(chat_id)
            .bind(chat_id)
            .execute(&mut *tx)
            .await?;
        self.add_participant_tx(&mut tx, chat_id, created_by_id)
            .await?;

        // Сессия: привязка к проекту напрямую + воркстейшн + владелец.
        sqlx::query(
            "INSERT INTO sessions (chat_id, project_id, workstation_id, owner_id) VALUES (?, ?, ?, ?)",
        )
        .bind(chat_id)
        .bind(project_id)
        .bind(workstation_id)
        .bind(created_by_id)
        .execute(&mut *tx)
        .await?;

        // Воркстейшн ссылается на текущую сессию.
        sqlx::query("UPDATE workstations SET current_session_id = ? WHERE id = ?")
            .bind(chat_id)
            .bind(workstation_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        // Ручное восстановление после падения: если на упавшей станции того же
        // проекта есть незакрытая сессия — новая считается её продолжением.
        if let Some(interrupted) = self.interrupted_session_for_project(project_id).await? {
            self.set_continues_session(chat_id, interrupted.session_id)
                .await?;
        }

        Ok(self.get_chat(chat_id).await?.unwrap())
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

    /// Воркстейшн корневого чата, к которому принадлежит чат. Поднимается по
    /// `parent_id` до корня, затем берёт `workstation_id` из `sessions`.
    pub async fn root_workstation_id(&self, chat_id: i64) -> Result<Option<i64>, sqlx::Error> {
        let mut id = chat_id;
        let mut seen = 0;
        loop {
            let row = sqlx::query("SELECT root_id, parent_id FROM chats WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
            let Some(r) = row else { return Ok(None) };
            if seen > max_chat_level() {
                return Ok(None);
            }
            seen += 1;
            let root_id: i64 = r.get("root_id");
            let parent_id: Option<i64> = r.get("parent_id");
            if parent_id.is_none() || root_id == id {
                let row = sqlx::query("SELECT workstation_id FROM sessions WHERE chat_id = ?")
                    .bind(root_id)
                    .fetch_optional(&self.pool)
                    .await?;
                return Ok(row.and_then(|r| r.get::<Option<i64>, _>("workstation_id")));
            }
            id = parent_id.unwrap();
        }
    }

    /// Проект, к которому принадлежит чат: поднимается до корня и берёт
    /// `sessions.project_id` напрямую (без транзита через воркстейшн).
    pub async fn project_id_for_chat(&self, chat_id: i64) -> Result<Option<i64>, sqlx::Error> {
        let mut id = chat_id;
        let mut seen = 0;
        loop {
            let row = sqlx::query("SELECT root_id, parent_id FROM chats WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
            let Some(r) = row else { return Ok(None) };
            if seen > max_chat_level() {
                return Ok(None);
            }
            seen += 1;
            let root_id: i64 = r.get("root_id");
            let parent_id: Option<i64> = r.get("parent_id");
            if parent_id.is_none() || root_id == id {
                let row = sqlx::query("SELECT project_id FROM sessions WHERE chat_id = ?")
                    .bind(root_id)
                    .fetch_optional(&self.pool)
                    .await?;
                return Ok(row.and_then(|r| r.get::<Option<i64>, _>("project_id")));
            }
            id = parent_id.unwrap();
        }
    }
}
