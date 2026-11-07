use super::*;
use sqlx::Row;

impl TraceStore {
    // === Каталог MCP-серверов ===

    fn mcp_row(r: &sqlx::sqlite::SqliteRow) -> McpServer {
        McpServer {
            id: r.get("id"),
            name: r.get("name"),
            transport: r.get("transport"),
            url: r.get("url"),
            command: r.get("command"),
            api_key: r.get("api_key"),
        }
    }

    pub async fn list_mcp_servers(&self) -> Result<Vec<McpServer>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, name, transport, url, command, api_key FROM mcp_servers ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(Self::mcp_row).collect())
    }

    pub async fn get_mcp_server(&self, id: i64) -> Result<Option<McpServer>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, name, transport, url, command, api_key FROM mcp_servers WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(Self::mcp_row))
    }

    /// MCP-серверы по именам (порядок имён; неизвестные пропускаются).
    pub async fn mcp_servers_by_names(
        &self,
        names: &[String],
    ) -> Result<Vec<McpServer>, sqlx::Error> {
        let all = self.list_mcp_servers().await?;
        Ok(names
            .iter()
            .filter_map(|n| all.iter().find(|s| &s.name == n).cloned())
            .collect())
    }

    /// Занято ли имя другим MCP-сервером.
    pub async fn mcp_server_name_taken(
        &self,
        name: &str,
        exclude_id: i64,
    ) -> Result<bool, sqlx::Error> {
        let row = sqlx::query("SELECT COUNT(*) AS c FROM mcp_servers WHERE name = ? AND id != ?")
            .bind(name)
            .bind(exclude_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get::<i64, _>("c") > 0)
    }

    pub async fn create_mcp_server(&self, spec: &McpServerSpec) -> Result<i64, sqlx::Error> {
        let result = sqlx::query(
            "INSERT INTO mcp_servers (name, transport, url, command, api_key) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(spec.name.trim())
        .bind(&spec.transport)
        .bind(spec.url.trim())
        .bind(spec.command.trim())
        .bind(&spec.api_key)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Изменить MCP-сервер. false — сервера нет. Агенты ссылаются по id —
    /// переименование привязки не рвёт.
    pub async fn update_mcp_server(
        &self,
        id: i64,
        spec: &McpServerSpec,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE mcp_servers SET name = ?, transport = ?, url = ?, command = ?, api_key = ? WHERE id = ?",
        )
        .bind(spec.name.trim())
        .bind(&spec.transport)
        .bind(spec.url.trim())
        .bind(spec.command.trim())
        .bind(&spec.api_key)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Удалить MCP-сервер: привязки агентов уходят каскадом.
    pub async fn delete_mcp_server(&self, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM mcp_servers WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
