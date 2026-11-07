use super::*;
use sqlx::Row;

impl TraceStore {
    // === Каталог способностей (скиллы и сокращения) ===

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
}

/// Запись каталога из строки SELECT (id, kind, name, content, deleted).
fn capability_from_row(r: &sqlx::sqlite::SqliteRow) -> CapabilityItem {
    let kind: String = r.get("kind");
    CapabilityItem {
        id: r.get("id"),
        kind: match kind.as_str() {
            "shortcut" => CapabilityKind::Shortcut,
            _ => CapabilityKind::Skill,
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
