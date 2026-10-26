use super::rows::*;
use super::*;

impl ChatStore {
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
}
