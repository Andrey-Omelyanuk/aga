use super::*;
use chrono::{DateTime, Utc};
use sqlx::Row;

impl TraceStore {
    // === Методы для управления проектами ===

    /// Создать или получить проект по git-URL.
    pub async fn upsert_project(&self, git_url: &str) -> Result<i64, sqlx::Error> {
        // Пробуем найти существующий проект
        let existing = sqlx::query("SELECT id FROM projects WHERE git_url = ?")
            .bind(git_url)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = existing {
            let id: i64 = row.get("id");
            // Обновляем updated_at
            sqlx::query("UPDATE projects SET updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(id)
                .execute(&self.pool)
                .await?;
            Ok(id)
        } else {
            // Создаём новый проект
            let result = sqlx::query("INSERT INTO projects (git_url) VALUES (?)")
                .bind(git_url)
                .execute(&self.pool)
                .await?;
            Ok(result.last_insert_rowid())
        }
    }

    /// Получить проект по ID
    pub async fn get_project(&self, project_id: i64) -> Result<Option<Project>, sqlx::Error> {
        let row =
            sqlx::query("SELECT id, git_url, created_at, updated_at FROM projects WHERE id = ?")
                .bind(project_id)
                .fetch_optional(&self.pool)
                .await?;

        match row {
            Some(r) => {
                let id: i64 = r.get("id");
                let git_url: String = r.get("git_url");
                let created_at: DateTime<Utc> = r.get("created_at");
                let updated_at: DateTime<Utc> = r.get("updated_at");
                Ok(Some(Project {
                    id,
                    git_url,
                    created_at,
                    updated_at,
                }))
            }
            None => Ok(None),
        }
    }

    /// Получить проект по git-URL
    #[allow(dead_code)]
    pub async fn get_project_by_url(&self, git_url: &str) -> Result<Option<Project>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, git_url, created_at, updated_at FROM projects WHERE git_url = ?",
        )
        .bind(git_url)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => {
                let id: i64 = r.get("id");
                let git_url: String = r.get("git_url");
                let created_at: DateTime<Utc> = r.get("created_at");
                let updated_at: DateTime<Utc> = r.get("updated_at");
                Ok(Some(Project {
                    id,
                    git_url,
                    created_at,
                    updated_at,
                }))
            }
            None => Ok(None),
        }
    }

    /// Получить все проекты
    pub async fn get_all_projects(&self) -> Result<Vec<Project>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, git_url, created_at, updated_at FROM projects ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut projects = Vec::new();
        for row in rows {
            let id: i64 = row.get("id");
            let git_url: String = row.get("git_url");
            let created_at: DateTime<Utc> = row.get("created_at");
            let updated_at: DateTime<Utc> = row.get("updated_at");
            projects.push(Project {
                id,
                git_url,
                created_at,
                updated_at,
            });
        }
        Ok(projects)
    }

    /// Удалить проект
    pub async fn delete_project(&self, project_id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(project_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
