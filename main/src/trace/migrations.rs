//! Миграции старых БД: досоздание и переименование столбцов, которых
//! `CREATE TABLE IF NOT EXISTS` у существующих таблиц не добавляет.
//! Вызываются из `TraceStore::new`.

use sqlx::{Row, SqlitePool};

/// Миграция старых БД: история каталога хранит содержимое записи после
/// действия (`content`) — по нему страница истории строит дифф. Старые записи
/// получают пустое содержимое (снапшотов тогда не было).
pub(super) async fn migrate_capability_history_content(
    pool: &SqlitePool,
) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(capability_history)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    if !cols.iter().any(|c| c == "content") {
        sqlx::query("ALTER TABLE capability_history ADD COLUMN content TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Перевести старые БД на новую модель каталога: у записей одно содержимое
/// (`content`) и мягкое удаление (`deleted`). Содержимое берётся из последней
/// версии; UNIQUE(kind, name) в старой схеме уже есть и сохраняется.
pub(super) async fn migrate_capabilities_columns(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(capabilities)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    if !cols.iter().any(|c| c == "content") {
        sqlx::query("ALTER TABLE capabilities ADD COLUMN content TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await?;
        // Наполнение из последней версии. Таблицы версий может не быть вообще
        // (старая схема без версий) — тогда остаётся пустое содержимое; если
        // версий для записи нет, COALESCE не даёт NULL нарушить NOT NULL.
        let has_versions = sqlx::query(
            "SELECT COUNT(*) AS c FROM sqlite_master WHERE type = 'table' AND name = 'capability_versions'",
        )
        .fetch_one(pool)
        .await?
        .get::<i64, _>("c")
            > 0;
        if has_versions {
            sqlx::query(
                "UPDATE capabilities SET content = COALESCE((
                    SELECT v.content FROM capability_versions v
                    WHERE v.capability_id = capabilities.id
                    ORDER BY v.id DESC LIMIT 1
                ), '')",
            )
            .execute(pool)
            .await?;
        }
    }
    if !cols.iter().any(|c| c == "deleted") {
        sqlx::query("ALTER TABLE capabilities ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0")
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Удалить записи каталога команд (`kind='command'`): вид команды удалён,
/// старые записи и их история/привязки к агентам не нужны. Каскад по FK
/// чистит `capability_history` и `agent_capabilities`. Идемпотентно.
pub(super) async fn migrate_remove_command_capabilities(
    pool: &SqlitePool,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM capabilities WHERE kind = 'command'")
        .execute(pool)
        .await?;
    Ok(())
}

/// Переименовать `projects.compose_path` в `git_url` у старых БД.
/// SQLite не поддерживает `IF EXISTS` для ALTER, поэтому столбец проверяется
/// через `PRAGMA table_info`.
pub(super) async fn migrate_projects_git_url(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(projects)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    let has_compose = cols.iter().any(|c| c == "compose_path");
    let has_git = cols.iter().any(|c| c == "git_url");
    if has_compose && !has_git {
        sqlx::query("ALTER TABLE projects RENAME COLUMN compose_path TO git_url")
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Переименовать `agents.allowed_commands` в `agents.tools` у старых БД:
/// «команды агента» стали «инструментами» — отдельным списком без версий.
pub(super) async fn migrate_agents_tools_column(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(agents)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    let has_allowed = cols.iter().any(|c| c == "allowed_commands");
    let has_tools = cols.iter().any(|c| c == "tools");
    if has_allowed && !has_tools {
        sqlx::query("ALTER TABLE agents RENAME COLUMN allowed_commands TO tools")
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Миграция старых БД: у агента набора появилось выбранное подключение к LLM
/// (`llm_id`). Своей модели и температуры у агента больше нет — столбцы
/// model/temperature старых БД остаются, но кодом не используются.
pub(super) async fn migrate_agents_llm_column(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(agents)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    if !cols.iter().any(|c| c == "llm_id") {
        sqlx::query("ALTER TABLE agents ADD COLUMN llm_id INTEGER")
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Миграция старых БД: привязка агента к пользователю чата (`listen_user_id`).
pub(super) async fn migrate_agents_listen_user_column(
    pool: &SqlitePool,
) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(agents)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    if !cols.iter().any(|c| c == "listen_user_id") {
        sqlx::query("ALTER TABLE agents ADD COLUMN listen_user_id INTEGER")
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Миграция старых БД: у подключения к LLM появились модель (`model`), флаг
/// дефолта (`is_default`) и режим инструментов (`native_tools`, по умолчанию 1). Модель больше не из env — она живёт в подключении;
/// старые строки получают пустую модель (их правит пользователь на странице).
pub(super) async fn migrate_llm_connection_columns(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(llm_connections)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    if !cols.iter().any(|c| c == "model") {
        sqlx::query("ALTER TABLE llm_connections ADD COLUMN model TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await?;
    }
    if !cols.iter().any(|c| c == "is_default") {
        sqlx::query("ALTER TABLE llm_connections ADD COLUMN is_default INTEGER NOT NULL DEFAULT 0")
            .execute(pool)
            .await?;
    }
    if !cols.iter().any(|c| c == "native_tools") {
        sqlx::query(
            "ALTER TABLE llm_connections ADD COLUMN native_tools INTEGER NOT NULL DEFAULT 1",
        )
        .execute(pool)
        .await?;
    }
    if !cols.iter().any(|c| c == "max_tokens") {
        sqlx::query("ALTER TABLE llm_connections ADD COLUMN max_tokens INTEGER")
            .execute(pool)
            .await?;
    }
    if !cols.iter().any(|c| c == "temperature") {
        sqlx::query("ALTER TABLE llm_connections ADD COLUMN temperature REAL")
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Токены LLM за задачу (`tasks.prompt_tokens`/`completion_tokens`) —
/// добавляются в старые БД.
pub(super) async fn migrate_task_usage_columns(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(tasks)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    for col in ["prompt_tokens", "completion_tokens"] {
        if !cols.iter().any(|c| c == col) {
            sqlx::query(&format!(
                "ALTER TABLE tasks ADD COLUMN {col} INTEGER NOT NULL DEFAULT 0"
            ))
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// Миграция старых БД: у human_requests появились привязка к чату —
/// сообщение вопроса (`chat_id`, `question_message_id`) и имя агента,
/// задавшего вопрос (ответ ищется по «ответу на сообщение вопроса»).
pub(super) async fn migrate_human_request_chat_columns(
    pool: &SqlitePool,
) -> Result<(), sqlx::Error> {
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(human_requests)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    if !cols.iter().any(|c| c == "chat_id") {
        sqlx::query("ALTER TABLE human_requests ADD COLUMN chat_id INTEGER")
            .execute(pool)
            .await?;
    }
    if !cols.iter().any(|c| c == "question_message_id") {
        sqlx::query("ALTER TABLE human_requests ADD COLUMN question_message_id INTEGER")
            .execute(pool)
            .await?;
    }
    if !cols.iter().any(|c| c == "agent_name") {
        sqlx::query("ALTER TABLE human_requests ADD COLUMN agent_name TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await?;
    }
    Ok(())
}
