use super::*;
use sqlx::Row;

impl TraceStore {
    // === Методы для управления подключениями к LLM ===

    fn llm_row(&self, r: &sqlx::sqlite::SqliteRow) -> LlmConnection {
        LlmConnection {
            id: r.get("id"),
            name: r.get("name"),
            api_url: r.get("api_url"),
            api_key: r.get("api_key"),
            model_name: r.get("model"),
            is_default: r.get::<i64, _>("is_default") != 0,
            native_tools: r.get::<i64, _>("native_tools") != 0,
            max_tokens: r.get("max_tokens"),
            temperature: r.get("temperature"),
        }
    }

    /// Создать подключение к LLM: название, url API, ключ доступа и модель.
    pub async fn create_llm_connection(
        &self,
        spec: &LlmConnectionSpec,
    ) -> Result<i64, sqlx::Error> {
        let result = sqlx::query(
            "INSERT INTO llm_connections (name, api_url, api_key, model, native_tools, max_tokens, temperature) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&spec.name)
        .bind(&spec.api_url)
        .bind(&spec.api_key)
        .bind(&spec.model_name)
        .bind(spec.native_tools)
        .bind(spec.max_tokens)
        .bind(spec.temperature)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Изменить подключение: название, url API, ключ, модель. Возвращает false,
    /// если подключения нет.
    pub async fn update_llm_connection(
        &self,
        id: i64,
        spec: &LlmConnectionSpec,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE llm_connections SET name = ?, api_url = ?, api_key = ?, model = ?, native_tools = ?, max_tokens = ?, temperature = ? WHERE id = ?",
        )
        .bind(&spec.name)
        .bind(&spec.api_url)
        .bind(&spec.api_key)
        .bind(&spec.model_name)
        .bind(spec.native_tools)
        .bind(spec.max_tokens)
        .bind(spec.temperature)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Удалить подключение. Агенты, ссылавшиеся на него, остаются без
    /// подключения (llm_id обнуляется каскадом на новых БД, на старых —
    /// ссылка просто не находится) и, как агенты без подключения, ходят к
    /// дефолтной LLM. Было дефолтным — дефолт исчезает вместе с ним.
    pub async fn delete_llm_connection(&self, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM llm_connections WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_llm_connection(&self, id: i64) -> Result<Option<LlmConnection>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, name, api_url, api_key, model, is_default, native_tools, max_tokens, temperature
             FROM llm_connections WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| self.llm_row(&r)))
    }

    pub async fn list_llm_connections(&self) -> Result<Vec<LlmConnection>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, name, api_url, api_key, model, is_default, native_tools, max_tokens, temperature
             FROM llm_connections ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(|r| self.llm_row(r)).collect())
    }

    /// Сделать подключение дефолтной LLM: дефолт снимается с остальных (одна
    /// дефолтная LLM). Возвращает false, если подключения нет.
    pub async fn set_default_llm(&self, id: i64) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE llm_connections SET is_default = 0 WHERE is_default = 1")
            .execute(&mut *tx)
            .await?;
        let result = sqlx::query("UPDATE llm_connections SET is_default = 1 WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    /// Снять выбор дефолтной LLM: ни одна LLM не дефолтная, агенты без своего
    /// подключения остаются без LLM.
    pub async fn clear_default_llm(&self) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE llm_connections SET is_default = 0 WHERE is_default = 1")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Дефолтная LLM (одна). None — дефолт не выбран.
    pub async fn default_llm_connection(&self) -> Result<Option<LlmConnection>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, name, api_url, api_key, model, is_default, native_tools, max_tokens, temperature
             FROM llm_connections WHERE is_default = 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| self.llm_row(&r)))
    }
}
