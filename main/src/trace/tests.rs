use super::*;
use std::collections::HashMap;
use uuid::Uuid;

fn temp_db_path() -> (String, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("aga_trace_test_{}.db", Uuid::new_v4()));
    (path.to_string_lossy().into_owned(), path)
}

#[tokio::test]
async fn project_registered_by_git_url() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let url = "git@github.com:acme/proj.git";
    let id = store.upsert_project(url).await.unwrap();
    let project = store.get_project(id).await.unwrap().unwrap();
    assert_eq!(project.git_url, url);
    let again = store.upsert_project(url).await.unwrap();
    assert_eq!(again, id);
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn migrates_compose_path_to_git_url() {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    let (path, file) = temp_db_path();
    let options = SqliteConnectOptions::from_str(&path)
        .unwrap()
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE projects (id INTEGER PRIMARY KEY AUTOINCREMENT, compose_path TEXT NOT NULL UNIQUE, created_at DATETIME DEFAULT CURRENT_TIMESTAMP, updated_at DATETIME DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO projects (compose_path) VALUES ('/old/host/path')")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let store = TraceStore::new(&path).await.unwrap();
    let old = store.get_project(1).await.unwrap().unwrap();
    assert_eq!(old.git_url, "/old/host/path");

    let id = store
        .upsert_project("https://example.com/r.git")
        .await
        .unwrap();
    let project = store.get_project(id).await.unwrap().unwrap();
    assert_eq!(project.git_url, "https://example.com/r.git");

    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn migrates_capabilities_to_single_content_and_soft_delete() {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    let (path, file) = temp_db_path();
    let options = SqliteConnectOptions::from_str(&path)
        .unwrap()
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .unwrap();
    // Старая схема: capability_versions + UNIQUE(kind, name).
    sqlx::query(
        "CREATE TABLE capabilities (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, name TEXT NOT NULL, UNIQUE (kind, name))",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE capability_versions (id INTEGER PRIMARY KEY AUTOINCREMENT, capability_id INTEGER NOT NULL, version TEXT NOT NULL, content TEXT NOT NULL)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO capabilities (kind, name) VALUES ('skill', 'review')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO capability_versions (capability_id, version, content) VALUES (1, 'v1', 'старое содержимое')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Запись без версий: миграция не должна падать — остаётся пустое содержимое.
    sqlx::query("INSERT INTO capabilities (kind, name) VALUES ('skill', 'orphan')")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    // Открытие мигрирует: содержимое из последней версии, запись активна.
    let store = TraceStore::new(&path).await.unwrap();
    let item = store.get_capability(1).await.unwrap().unwrap();
    assert_eq!(item.content, "старое содержимое");
    assert!(!item.deleted);
    assert_eq!(item.name, "review");
    let orphan = store.get_capability(2).await.unwrap().unwrap();
    assert_eq!(orphan.content, "");
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn migrate_removes_legacy_command_capabilities() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    // «Команды» старых БД: запись каталога kind='command' с историей и
    // привязкой к агенту. Создаём напрямую (вида в коде больше нет).
    let skill = store
        .create_capability(CapabilityKind::Skill, "review", "v1", 1, "alice")
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO capabilities (kind, name, content) VALUES ('command', 'deploy', 'c1')",
    )
    .execute(&store.pool)
    .await
    .unwrap();
    let cmd_id: i64 = sqlx::query_scalar("SELECT id FROM capabilities WHERE kind = 'command'")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO capability_history (capability_id, action, actor_id, actor_name, content)
         VALUES (?, 'create', 1, 'alice', 'c1')",
    )
    .bind(cmd_id)
    .execute(&store.pool)
    .await
    .unwrap();
    let set_id = store
        .create_agent_set(
            "ops",
            &[AgentSpec {
                name: "dev".to_string(),
                description: "".to_string(),
                tools: vec![],
                max_iterations: 3,
                llm_id: None,
                parent: None,
                skills: vec![cap("review")],
                listen_user_id: None,
                mcp: vec![],
            }],
        )
        .await
        .unwrap();
    sqlx::query("INSERT INTO agent_capabilities (agent_id, capability_id) VALUES (?, ?)")
        .bind(set_id)
        .bind(cmd_id)
        .execute(&store.pool)
        .await
        .unwrap();
    drop(store);

    // Повторное открытие БД прогоняет миграцию: команды и всё связанное
    // (история, привязки) удаляются, скиллы и сокращения целы.
    let store = TraceStore::new(&path).await.unwrap();
    let cmd_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM capabilities WHERE kind = 'command'")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(cmd_count, 0);
    let history_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM capability_history WHERE capability_id = ?")
            .bind(cmd_id)
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(history_count, 0);
    let link_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM agent_capabilities WHERE capability_id = ?")
            .bind(cmd_id)
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(link_count, 0);
    let skill_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM capabilities WHERE kind = 'skill'")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(skill_count, 1);
    assert_eq!(
        store.get_capability(skill).await.unwrap().unwrap().name,
        "review"
    );
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn migrates_capability_history_content_column() {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    let (path, file) = temp_db_path();
    let options = SqliteConnectOptions::from_str(&path)
        .unwrap()
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .unwrap();
    // Старая схема: история без снапшотов содержимого.
    sqlx::query(
        "CREATE TABLE capability_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            capability_id INTEGER NOT NULL,
            action TEXT NOT NULL,
            actor_id INTEGER NOT NULL,
            actor_name TEXT NOT NULL DEFAULT '',
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            detail TEXT
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO capability_history (capability_id, action, actor_id, actor_name) VALUES (1, 'update', 2, 'alice')",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    // Открытие добавляет столбец content: старая запись получает пустое
    // содержимое (снапшота не было) и не ломает чтение.
    let store = TraceStore::new(&path).await.unwrap();
    let history = store.capability_history(1).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].action, CapabilityAction::Update);
    assert_eq!(history[0].content, "");
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn participant_sees_all_projects() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    store
        .upsert_project("https://example.com/one.git")
        .await
        .unwrap();
    store
        .upsert_project("https://example.com/two.git")
        .await
        .unwrap();
    let projects = store.get_all_projects().await.unwrap();
    assert_eq!(projects.len(), 2);
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn created_project_visible_to_all_participants() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let id = store
        .upsert_project("https://example.com/new.git")
        .await
        .unwrap();
    // Список проектов не фильтруется по пользователю: созданный проект
    // виден сразу всем участникам.
    let projects = store.get_all_projects().await.unwrap();
    assert!(projects.iter().any(|p| p.id == id));
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

fn spec(name: &str, parent: Option<&str>) -> AgentSpec {
    AgentSpec {
        name: name.to_string(),
        description: format!("Правила {name}"),
        tools: vec!["echo".to_string()],
        max_iterations: 3,
        llm_id: None,
        parent: parent.map(|s| s.to_string()),
        skills: vec![],
        listen_user_id: None,
        mcp: vec![],
    }
}

#[tokio::test]
async fn one_agent_set_attaches_to_many_projects() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let pa = store
        .upsert_project("https://example.com/a.git")
        .await
        .unwrap();
    let pb = store
        .upsert_project("https://example.com/b.git")
        .await
        .unwrap();
    let set_id = store
        .create_agent_set("ops", &[spec("dev", None)])
        .await
        .unwrap();
    store.attach_agent_set(pa, set_id).await.unwrap();
    store.attach_agent_set(pb, set_id).await.unwrap();
    assert_eq!(
        store.get_project_agent_set(pa).await.unwrap().unwrap().id,
        set_id
    );
    assert_eq!(
        store.get_project_agent_set(pb).await.unwrap().unwrap().id,
        set_id
    );
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn each_agent_keeps_own_rules_and_llm() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let conn = store
        .create_llm_connection(&LlmConnectionSpec {
            name: "conn".to_string(),
            api_url: "http://llm/v1".to_string(),
            api_key: Some("key".to_string()),
            model_name: "qwen3:0.6b".to_string(),
            native_tools: true,
            max_tokens: None,
            temperature: None,
        })
        .await
        .unwrap();
    let s1 = AgentSpec {
        name: "dev".to_string(),
        description: "Правила разработчика".to_string(),
        tools: vec!["git".to_string(), "make".to_string()],
        max_iterations: 2,
        llm_id: Some(conn),
        parent: None,
        skills: vec![],
        listen_user_id: None,
        mcp: vec![],
    };
    let s2 = AgentSpec {
        name: "deploy".to_string(),
        description: "Правила деплоера".to_string(),
        tools: vec!["docker".to_string()],
        max_iterations: 9,
        llm_id: None,
        parent: None,
        skills: vec![],
        listen_user_id: None,
        mcp: vec![],
    };
    let set_id = store.create_agent_set("ops", &[s1, s2]).await.unwrap();
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
    let deploy = set.agents.iter().find(|a| a.name == "deploy").unwrap();
    assert_eq!(dev.description, "Правила разработчика");
    assert_eq!(dev.tools, vec!["git".to_string(), "make".to_string()]);
    assert_eq!(dev.max_iterations, 2);
    assert_eq!(dev.llm_id, Some(conn));
    assert_eq!(deploy.tools, vec!["docker".to_string()]);
    assert_eq!(deploy.max_iterations, 9);
    assert!(deploy.llm_id.is_none());
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn updated_connection_changes_url_and_key_for_agents() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let conn = store
        .create_llm_connection(&LlmConnectionSpec {
            name: "conn".to_string(),
            api_url: "http://old/v1".to_string(),
            api_key: Some("old-key".to_string()),
            model_name: "qwen3:0.6b".to_string(),
            native_tools: true,
            max_tokens: None,
            temperature: None,
        })
        .await
        .unwrap();
    let set_id = store
        .create_agent_set(
            "ops",
            &[AgentSpec {
                name: "dev".to_string(),
                description: "Правила".to_string(),
                tools: vec![],
                max_iterations: 3,
                llm_id: Some(conn),
                parent: None,
                skills: vec![],
                listen_user_id: None,
                mcp: vec![],
            }],
        )
        .await
        .unwrap();
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
    let cfg = store.llm_config_for(dev).await.unwrap();
    assert_eq!(cfg.api_url.as_deref(), Some("http://old/v1"));
    assert_eq!(cfg.api_key.as_deref(), Some("old-key"));
    assert_eq!(cfg.model.as_deref(), Some("qwen3:0.6b"));
    // Правка подключения — без перенастройки агентов: тот же llm_id, новый url/ключ.
    store
        .update_llm_connection(
            conn,
            &LlmConnectionSpec {
                name: "conn".to_string(),
                api_url: "http://new/v1".to_string(),
                api_key: Some("new-key".to_string()),
                model_name: "qwen3:1b".to_string(),
                native_tools: true,
                max_tokens: None,
                temperature: None,
            },
        )
        .await
        .unwrap();
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
    assert_eq!(dev.llm_id, Some(conn));
    let cfg = store.llm_config_for(dev).await.unwrap();
    assert_eq!(cfg.api_url.as_deref(), Some("http://new/v1"));
    assert_eq!(cfg.api_key.as_deref(), Some("new-key"));
    assert_eq!(cfg.model.as_deref(), Some("qwen3:1b"));
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn deleted_default_connection_clears_default_and_agent_has_no_llm() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let conn = store
        .create_llm_connection(&LlmConnectionSpec {
            name: "conn".to_string(),
            api_url: "http://conn/v1".to_string(),
            api_key: Some("key".to_string()),
            model_name: "qwen3:0.6b".to_string(),
            native_tools: true,
            max_tokens: None,
            temperature: None,
        })
        .await
        .unwrap();
    store.set_default_llm(conn).await.unwrap();
    let set_id = store
        .create_agent_set(
            "ops",
            &[AgentSpec {
                name: "dev".to_string(),
                description: "Правила".to_string(),
                tools: vec![],
                max_iterations: 3,
                llm_id: Some(conn),
                parent: None,
                skills: vec![],
                listen_user_id: None,
                mcp: vec![],
            }],
        )
        .await
        .unwrap();
    store.delete_llm_connection(conn).await.unwrap();
    // Удалённое подключение исчезает из списка, дефолт сбрасывается.
    assert!(store
        .list_llm_connections()
        .await
        .unwrap()
        .iter()
        .all(|c| c.id != conn));
    assert!(store.default_llm_connection().await.unwrap().is_none());
    // Агент, его использовавший, остаётся без подключения — LLM нет.
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
    let cfg = store.llm_config_for(dev).await.unwrap();
    assert!(cfg.api_url.is_none());
    assert!(cfg.api_key.is_none());
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn agent_without_connection_uses_default_llm() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let conn = store
        .create_llm_connection(&LlmConnectionSpec {
            name: "conn".to_string(),
            api_url: "http://default/v1".to_string(),
            api_key: Some("default-key".to_string()),
            model_name: "qwen3:0.6b".to_string(),
            native_tools: true,
            max_tokens: None,
            temperature: None,
        })
        .await
        .unwrap();
    store.set_default_llm(conn).await.unwrap();
    let set_id = store
        .create_agent_set(
            "ops",
            &[AgentSpec {
                name: "dev".to_string(),
                description: "Правила".to_string(),
                tools: vec![],
                max_iterations: 3,
                llm_id: Some(conn),
                parent: None,
                skills: vec![],
                listen_user_id: None,
                mcp: vec![],
            }],
        )
        .await
        .unwrap();
    // Агент без своего подключения ходит к дефолтной LLM: url, ключ и модель.
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
    let cfg = store.llm_config_for(dev).await.unwrap();
    assert_eq!(cfg.api_url.as_deref(), Some("http://default/v1"));
    assert_eq!(cfg.api_key.as_deref(), Some("default-key"));
    assert_eq!(cfg.model.as_deref(), Some("qwen3:0.6b"));
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn own_connection_wins_over_default() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let default = store
        .create_llm_connection(&LlmConnectionSpec {
            name: "default".to_string(),
            api_url: "http://default/v1".to_string(),
            api_key: None,
            model_name: "default-model".to_string(),
            native_tools: true,
            max_tokens: None,
            temperature: None,
        })
        .await
        .unwrap();
    store.set_default_llm(default).await.unwrap();
    let own = store
        .create_llm_connection(&LlmConnectionSpec {
            name: "own".to_string(),
            api_url: "http://own/v1".to_string(),
            api_key: Some("own-key".to_string()),
            model_name: "own-model".to_string(),
            native_tools: true,
            max_tokens: None,
            temperature: None,
        })
        .await
        .unwrap();
    let set_id = store
        .create_agent_set(
            "ops",
            &[AgentSpec {
                name: "dev".to_string(),
                description: "Правила".to_string(),
                tools: vec![],
                max_iterations: 3,
                llm_id: Some(own),
                parent: None,
                skills: vec![],
                listen_user_id: None,
                mcp: vec![],
            }],
        )
        .await
        .unwrap();
    // У агента со своим подключением дефолтная LLM не влияет.
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
    let cfg = store.llm_config_for(dev).await.unwrap();
    assert_eq!(cfg.api_url.as_deref(), Some("http://own/v1"));
    assert_eq!(cfg.api_key.as_deref(), Some("own-key"));
    assert_eq!(cfg.model.as_deref(), Some("own-model"));
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn setting_default_moves_it_and_clearing_removes_it() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let a = store
        .create_llm_connection(&LlmConnectionSpec {
            name: "a".to_string(),
            api_url: "http://a/v1".to_string(),
            api_key: None,
            model_name: "a-model".to_string(),
            native_tools: true,
            max_tokens: None,
            temperature: None,
        })
        .await
        .unwrap();
    let b = store
        .create_llm_connection(&LlmConnectionSpec {
            name: "b".to_string(),
            api_url: "http://b/v1".to_string(),
            api_key: None,
            model_name: "b-model".to_string(),
            native_tools: true,
            max_tokens: None,
            temperature: None,
        })
        .await
        .unwrap();
    assert!(store.default_llm_connection().await.unwrap().is_none());
    store.set_default_llm(a).await.unwrap();
    assert_eq!(store.default_llm_connection().await.unwrap().unwrap().id, a);
    // Новый дефолт снимает прежний: дефолтная LLM одна.
    store.set_default_llm(b).await.unwrap();
    assert_eq!(store.default_llm_connection().await.unwrap().unwrap().id, b);
    // Снять выбор: ни одна не дефолтная.
    store.clear_default_llm().await.unwrap();
    assert!(store.default_llm_connection().await.unwrap().is_none());
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn connection_keeps_model_and_default_flag() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let conn = store
        .create_llm_connection(&LlmConnectionSpec {
            name: "conn".to_string(),
            api_url: "http://llm/v1".to_string(),
            api_key: Some("key".to_string()),
            model_name: "qwen3:0.6b".to_string(),
            native_tools: true,
            max_tokens: None,
            temperature: None,
        })
        .await
        .unwrap();
    store.set_default_llm(conn).await.unwrap();
    let listed = store.list_llm_connections().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].model_name, "qwen3:0.6b");
    assert!(listed[0].is_default);
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn agent_set_agents_form_tree_by_parent() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let set_id = store
        .create_agent_set(
            "tree",
            &[
                spec("src", None),
                spec("src/backend", Some("src")),
                spec("src/backend/api", Some("src/backend")),
            ],
        )
        .await
        .unwrap();
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    let by_name: std::collections::HashMap<&str, &AgentDef> =
        set.agents.iter().map(|a| (a.name.as_str(), a)).collect();
    let root = by_name["src"];
    let backend = by_name["src/backend"];
    let api = by_name["src/backend/api"];
    // Дерево повторяет иерархию: у папки — агент, у подпапок — его наследники.
    assert_eq!(backend.parent_id, Some(root.id));
    assert_eq!(api.parent_id, Some(backend.id));
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn replacing_agent_set_changes_project_agents() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let project = store
        .upsert_project("https://example.com/r.git")
        .await
        .unwrap();
    let set_a = store
        .create_agent_set("set-a", &[spec("dev-a", None)])
        .await
        .unwrap();
    let set_b = store
        .create_agent_set("set-b", &[spec("dev-b", None)])
        .await
        .unwrap();
    store.attach_agent_set(project, set_a).await.unwrap();
    assert_eq!(
        store
            .get_project_agent_set(project)
            .await
            .unwrap()
            .unwrap()
            .name,
        "set-a"
    );
    store.attach_agent_set(project, set_b).await.unwrap();
    let set = store.get_project_agent_set(project).await.unwrap().unwrap();
    assert_eq!(set.name, "set-b");
    assert!(set.agents.iter().all(|a| a.name == "dev-b"));
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn deleted_agent_set_disappears_from_projects() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let project = store
        .upsert_project("https://example.com/r.git")
        .await
        .unwrap();
    let set_id = store
        .create_agent_set("ops", &[spec("dev", None)])
        .await
        .unwrap();
    store.attach_agent_set(project, set_id).await.unwrap();
    store.delete_agent_set(set_id).await.unwrap();
    // Набор удалён — привязка проекта каскадно снята: работает только набор.
    assert!(store
        .get_project_agent_set(project)
        .await
        .unwrap()
        .is_none());
    assert!(store.get_agent_set(set_id).await.unwrap().is_none());
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

fn cap(name: &str) -> AgentCapability {
    AgentCapability {
        name: name.to_string(),
    }
}

#[tokio::test]
async fn agent_binds_to_listening_user_survives_update() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let mut dev = spec("dev", None);
    dev.listen_user_id = Some(42);
    let set_id = store.create_agent_set("ops", &[dev]).await.unwrap();
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    // Привязка слышна сразу после создания набора.
    assert_eq!(set.agents[0].listen_user_id, Some(42));
    let found = store
        .agent_listening_to(&set, 42)
        .expect("агент слушает 42");
    assert_eq!(found.name, "dev");
    // Чужой пользователь не триггерит этого агента.
    assert!(store.agent_listening_to(&set, 7).is_none());

    // Правка набора без привязки — она сброшена (состав меняется целиком).
    let mut api = spec("api", None);
    api.listen_user_id = Some(99);
    store.update_agent_set(set_id, "ops", &[api]).await.unwrap();
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    assert_eq!(set.agents[0].listen_user_id, Some(99));
    assert_eq!(store.agent_listening_to(&set, 99).unwrap().name, "api");
    assert!(store.agent_listening_to(&set, 42).is_none());
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn agent_without_binding_listens_to_nobody() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let set_id = store
        .create_agent_set("ops", &[spec("dev", None)])
        .await
        .unwrap();
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    assert_eq!(set.agents[0].listen_user_id, None);
    // Ни один пользователь не находит агента без привязки.
    assert!(store.agent_listening_to(&set, 1).is_none());
    assert!(store.agent_listening_to(&set, 42).is_none());
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn each_agent_owns_territory_by_its_tree_node() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let set_id = store
        .create_agent_set(
            "tree",
            &[
                spec("src", None),
                spec("src/backend", Some("src")),
                spec("src/backend/api", Some("src/backend")),
            ],
        )
        .await
        .unwrap();
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    let by_name: HashMap<&str, &AgentDef> =
        set.agents.iter().map(|a| (a.name.as_str(), a)).collect();
    // Территория агента — папка его узла в дереве набора: у папки свой
    // агент, подпапки — его наследники, их папки в его территорию не входят.
    // Корень дерева — корень проекта (папка ""): у него нет папки-родителя,
    // его территория — весь проект кроме папок наследников.
    assert_eq!(by_name["src"].territory.folder, "");
    assert_eq!(
        by_name["src"].territory.excludes,
        vec!["src/backend".to_string()]
    );
    assert_eq!(by_name["src/backend"].territory.folder, "src/backend");
    assert_eq!(
        by_name["src/backend"].territory.excludes,
        vec!["src/backend/api".to_string()]
    );
    // У листа наследников нет — территория вся его папка.
    assert_eq!(
        by_name["src/backend/api"].territory.folder,
        "src/backend/api"
    );
    assert!(by_name["src/backend/api"].territory.excludes.is_empty());
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn tools_are_plain_list_capabilities_have_single_content() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let skill = store
        .create_capability(CapabilityKind::Skill, "review", "v1", 1, "alice")
        .await
        .unwrap();
    let set_id = store
        .create_agent_set(
            "ops",
            &[AgentSpec {
                name: "dev".to_string(),
                description: "".to_string(),
                tools: vec!["git".to_string(), "make".to_string()],
                max_iterations: 3,
                llm_id: None,
                parent: None,
                skills: vec![cap("review")],
                listen_user_id: None,
                mcp: vec![],
            }],
        )
        .await
        .unwrap();
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
    // Инструменты — плоский список без версий.
    assert_eq!(dev.tools, vec!["git".to_string(), "make".to_string()]);
    assert_eq!(dev.skills.len(), 1);
    assert_eq!(dev.skills[0].name, "review");
    // У записи каталога одно текущее содержимое — версий и фиксации нет.
    let skill_item = store.get_capability(skill).await.unwrap().unwrap();
    assert_eq!(skill_item.content, "v1");
    assert!(!skill_item.deleted);
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn agent_uses_only_assigned_skills() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    store
        .create_capability(
            CapabilityKind::Skill,
            "review",
            "Описание review",
            1,
            "alice",
        )
        .await
        .unwrap();
    store
        .create_capability(
            CapabilityKind::Skill,
            "polish",
            "Описание polish",
            1,
            "alice",
        )
        .await
        .unwrap();
    let set_id = store
        .create_agent_set(
            "ops",
            &[AgentSpec {
                name: "dev".to_string(),
                description: "Правила".to_string(),
                tools: vec![],
                max_iterations: 3,
                llm_id: None,
                parent: None,
                skills: vec![cap("review")],
                listen_user_id: None,
                mcp: vec![],
            }],
        )
        .await
        .unwrap();
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
    assert_eq!(dev.skills, vec![cap("review")]);
    let prompt = store.agent_prompt(dev).await.unwrap();
    // В промпте только данные агенту скиллы.
    assert!(prompt.contains("Описание review"));
    assert!(!prompt.contains("polish"));
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn agent_always_uses_latest_capability_content() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let skill = store
        .create_capability(CapabilityKind::Skill, "review", "Формат диффов", 1, "alice")
        .await
        .unwrap();
    let set_id = store
        .create_agent_set(
            "ops",
            &[AgentSpec {
                name: "dev".to_string(),
                description: "Правила".to_string(),
                tools: vec![],
                max_iterations: 3,
                llm_id: None,
                parent: None,
                skills: vec![cap("review")],
                listen_user_id: None,
                mcp: vec![],
            }],
        )
        .await
        .unwrap();
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
    assert!(store
        .agent_prompt(dev)
        .await
        .unwrap()
        .contains("Формат диффов"));
    // Правка содержимого подхватывается без настройки агента: версий и
    // фиксации нет — агент всегда берёт единственное текущее содержимое.
    store
        .update_capability_content(skill, "Прогон тестов и правки", 1, "alice")
        .await
        .unwrap();
    let prompt = store.agent_prompt(dev).await.unwrap();
    assert!(prompt.contains("Прогон тестов и правки"));
    assert!(!prompt.contains("Формат диффов"));
    // Удалённая запись агенту не раскрывается.
    store.delete_capability(skill, 1, "alice").await.unwrap();
    assert!(!store
        .agent_prompt(dev)
        .await
        .unwrap()
        .contains("Прогон тестов и правки"));
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn capability_actions_written_to_history_with_author() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let skill = store
        .create_capability(CapabilityKind::Skill, "review", "v1", 7, "alice")
        .await
        .unwrap();
    store
        .update_capability_content(skill, "v2", 7, "alice")
        .await
        .unwrap();
    store
        .rename_capability(skill, "review2", 8, "bob")
        .await
        .unwrap();
    store.delete_capability(skill, 9, "carol").await.unwrap();
    let history = store.capability_history(skill).await.unwrap();
    // По порядку: создание, правка содержимого, переименование, удаление —
    // каждая правка с автором и временем.
    use CapabilityAction::*;
    let actions: Vec<CapabilityAction> = history.iter().map(|h| h.action).collect();
    assert_eq!(actions, vec![Create, Update, Rename, Delete]);
    assert_eq!(history[0].actor_id, 7);
    assert_eq!(history[2].actor_id, 8);
    assert_eq!(history[3].actor_id, 9);
    assert!(history[0].created_at <= history[3].created_at);
    // Каждая запись хранит содержимое после действия — по соседним записям
    // страница истории строит дифф: create -> v1, update -> v2, rename/delete
    // содержимое не меняют.
    let contents: Vec<&str> = history.iter().map(|h| h.content.as_str()).collect();
    assert_eq!(contents, vec!["v1", "v2", "v2", "v2"]);
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn capability_name_taken_includes_active_and_deleted_but_not_self() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let skill = store
        .create_capability(CapabilityKind::Skill, "review", "v1", 1, "alice")
        .await
        .unwrap();
    // Своё же имя не считается занятым — правка с тем же именем проходит.
    assert!(!store
        .capability_name_taken(CapabilityKind::Skill, "review", skill)
        .await
        .unwrap());
    // Другая активная запись с тем же именем — занято.
    assert!(
        store
            .create_capability(CapabilityKind::Skill, "taken", "x", 1, "alice")
            .await
            .unwrap()
            > 0
    );
    assert!(store
        .capability_name_taken(CapabilityKind::Skill, "taken", skill)
        .await
        .unwrap());
    // Мягко удалённая запись имя занимает.
    let taken_id = store
        .create_capability(CapabilityKind::Skill, "gone", "x", 1, "alice")
        .await
        .unwrap();
    store.delete_capability(taken_id, 1, "alice").await.unwrap();
    assert!(store
        .capability_name_taken(CapabilityKind::Skill, "gone", skill)
        .await
        .unwrap());
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn capability_renamed_and_deleted_with_history() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    let skill = store
        .create_capability(CapabilityKind::Skill, "review", "v1", 1, "alice")
        .await
        .unwrap();
    let set_id = store
        .create_agent_set(
            "ops",
            &[AgentSpec {
                name: "dev".to_string(),
                description: "".to_string(),
                tools: vec![],
                max_iterations: 3,
                llm_id: None,
                parent: None,
                skills: vec![cap("review")],
                listen_user_id: None,
                mcp: vec![],
            }],
        )
        .await
        .unwrap();
    // Переименование: данные агенту сохраняются, содержимое на месте.
    assert!(store
        .rename_capability(skill, "review2", 1, "alice")
        .await
        .unwrap());
    let item = store.get_capability(skill).await.unwrap().unwrap();
    assert_eq!(item.name, "review2");
    assert_eq!(item.content, "v1");
    assert!(!item.deleted);
    let set = store.get_agent_set(set_id).await.unwrap().unwrap();
    let dev = set.agents.iter().find(|a| a.name == "dev").unwrap();
    assert_eq!(dev.skills[0].name, "review2");
    // Переименование несуществующей способности — false.
    assert!(!store.rename_capability(999, "x", 1, "alice").await.unwrap());
    // Мягкое удаление: запись остаётся в «Удалённых» с сохранённой историей.
    assert!(store.delete_capability(skill, 1, "alice").await.unwrap());
    let item = store.get_capability(skill).await.unwrap().unwrap();
    assert!(item.deleted);
    assert!(store
        .list_capabilities(CapabilityKind::Skill, false)
        .await
        .unwrap()
        .is_empty());
    let deleted = store
        .list_deleted_capabilities(CapabilityKind::Skill)
        .await
        .unwrap();
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0].name, "review2");
    // История переживает удаление: создание, переименование, удаление.
    assert_eq!(store.capability_history(skill).await.unwrap().len(), 3);
    // Повторное удаление — false.
    assert!(!store.delete_capability(skill, 1, "alice").await.unwrap());
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn human_request_links_to_question_message_and_resolves() {
    let (path, file) = temp_db_path();
    let store = TraceStore::new(&path).await.unwrap();
    store.create_task("t1", "dev").await.unwrap();
    let id = store
        .create_human_request("t1", "Какой порт?")
        .await
        .unwrap();
    // Пока вопрос не опубликован в чат — по сообщению ничего не находится.
    assert!(store
        .pending_for_question_message(42)
        .await
        .unwrap()
        .is_none());
    assert!(store
        .link_human_request_chat(&id, 7, 42, "dev")
        .await
        .unwrap());
    let req = store
        .pending_for_question_message(42)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(req.id, id);
    assert_eq!(req.chat_id, 7);
    assert_eq!(req.agent_name, "dev");
    store.wait_human_task("t1").await.unwrap();
    assert_eq!(
        store.get_trace("t1").await.unwrap().unwrap().status,
        "waiting_human"
    );
    assert!(store.answer_human_request(&id, "8080").await.unwrap());
    // Закрытый запрос больше не находится как pending.
    assert!(store
        .pending_for_question_message(42)
        .await
        .unwrap()
        .is_none());
    // Повторный ответ ничего не меняет.
    assert!(!store.answer_human_request(&id, "ещё").await.unwrap());
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn migrates_human_request_chat_columns() {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    let (path, file) = temp_db_path();
    let options = SqliteConnectOptions::from_str(&path)
        .unwrap()
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE human_requests (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            question TEXT NOT NULL,
            answer TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            answered_at DATETIME
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO human_requests (id, task_id, question) VALUES ('hr1', 't9', 'Порт?')")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let store = TraceStore::new(&path).await.unwrap();
    assert!(store
        .link_human_request_chat("hr1", 3, 11, "dev")
        .await
        .unwrap());
    let req = store
        .pending_for_question_message(11)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(req.task_id, "t9");
    assert_eq!(req.chat_id, 3);
    let _ = std::fs::remove_file(format!("{}-wal", file.display()));
    let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn running_tasks_of_previous_process_become_interrupted() {
    let file = std::env::temp_dir().join(format!("aga_trace_int_{}.db", uuid::Uuid::new_v4()));
    let store = TraceStore::new(&file.to_string_lossy()).await.unwrap();
    store.create_task("t-run", "dev").await.unwrap();
    store.create_task("t-wait", "dev").await.unwrap();
    store.wait_human_task("t-wait").await.unwrap();
    store.create_task("t-done", "dev").await.unwrap();
    store.complete_task("t-done", "completed").await.unwrap();
    assert_eq!(store.interrupt_running_tasks().await.unwrap(), 1);
    // Ждущая ответа человека и завершённая не тронуты.
    assert_eq!(
        store.task_statuses().await.unwrap(),
        vec!["interrupted", "waiting_human", "completed"]
    );
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn task_accumulates_llm_token_usage() {
    let file = std::env::temp_dir().join(format!("aga_trace_use_{}.db", uuid::Uuid::new_v4()));
    let store = TraceStore::new(&file.to_string_lossy()).await.unwrap();
    store.create_task("t", "dev").await.unwrap();
    store.add_task_usage("t", 100, 20).await.unwrap();
    store.add_task_usage("t", 50, 5).await.unwrap();
    let trace = store.get_trace("t").await.unwrap().unwrap();
    assert_eq!((trace.prompt_tokens, trace.completion_tokens), (150, 25));
    let _ = std::fs::remove_file(&file);
}
