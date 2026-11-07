//! Наборы агентов.

use super::*;

/// Имя агента — путь папки: `..` и абсолютные пути песочница превратила бы
/// в выход за проект — набор с такими именами не сохраняется.
#[tokio::test]
async fn agent_set_with_traversal_agent_name_rejected() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    for bad in ["..", "src/../..", "/etc"] {
        let (status, _) = post_json(
            "/agent-sets",
            &alice,
            state.clone(),
            serde_json::json!({ "name": "ops", "agents": [agent_json(bad)] }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    let (status, body) = post_json(
        "/agent-sets",
        &alice,
        state.clone(),
        serde_json::json!({ "name": "ops", "agents": [agent_json("dev")] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let (status, _) = patch_json(
        &format!("/agent-sets/{id}"),
        &alice,
        state.clone(),
        serde_json::json!({ "name": "ops", "agents": [agent_json("../x")] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    cleanup(&file).await;
}

/// Агент с привязкой к пользователю чата: слушает его и отвечает от его
/// имени (настройка на странице агента).
fn bound_agent_json(name: &str, listen_user_id: i64) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "description": format!("Правила {name}"),
        "tools": ["git", "make"],
        "max_iterations": 3,
        "llm_id": null,
        "parent": null,
        "skills": [],
        "listen_user_id": listen_user_id
    })
}

#[tokio::test]
async fn agent_listen_binding_saved_and_stays_visible_via_api() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    let (status, body) = post_json(
        "/agent-sets",
        &headers,
        state.clone(),
        serde_json::json!({ "name": "ops", "agents": [bound_agent_json("dev", 42)] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let set_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    // Выбор слушаемого виден в деталях набора — страница настройки
    // показывает его после сохранения.
    let (status, body) = get(&format!("/agent-sets/{set_id}"), &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let detail: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(detail["agents"][0]["listen_user_id"], 42);
    // Правка состава с другой привязкой — сохраняется свежая.
    patch_json(
        &format!("/agent-sets/{set_id}"),
        &headers,
        state.clone(),
        serde_json::json!({ "name": "ops", "agents": [bound_agent_json("dev", 77)] }),
    )
    .await;
    let (_, body) = get(&format!("/agent-sets/{set_id}"), &headers, state).await;
    let detail: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(detail["agents"][0]["listen_user_id"], 77);
    cleanup(&file).await;
}

#[tokio::test]
async fn created_agent_set_listed_via_api() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    let (status, _) = post_json(
        "/agent-sets",
        &headers,
        state.clone(),
        serde_json::json!({ "name": "ops", "agents": [agent_json("dev")] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = get("/agent-sets", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("ops"));
    cleanup(&file).await;
}

#[tokio::test]
async fn agent_set_detail_includes_territory_skills_and_tools() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    // Каталог: скилл с единственным содержимым.
    let (status, body) = post_json(
        "/skills",
        &headers,
        state.clone(),
        serde_json::json!({
            "name": "review",
            "content": "Проверять диф"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let skill_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    // Набор с деревом и данными агенту скиллами (по имени, без версии).
    let (status, body) = post_json(
        "/agent-sets",
        &headers,
        state.clone(),
        serde_json::json!({
            "name": "ops",
            "agents": [{
                "name": "src",
                "description": "Разработка",
                "tools": ["git", "make"],
                "max_iterations": 3,
                "llm_id": null,
                "parent": null,
                "skills": [{"name": "review"}]
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let set_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let (status, body) = get(&format!("/agent-sets/{set_id}"), &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    // Состав набора: агенты, территория каждого, данные скиллы (по имени,
    // без версии), инструменты — всё в детали набора.
    assert!(body.contains("territory"));
    assert!(body.contains("folder"));
    assert!(body.contains("src"));
    assert!(body.contains("tools"));
    assert!(body.contains("git"));
    assert!(body.contains("skills"));
    assert!(body.contains("review"));
    assert!(!body.contains("commands"));
    assert!(!body.contains("pinned_version"));
    // Своей модели и температуры у агента больше нет — только подключение.
    assert!(!body.contains("\"model\""));
    assert!(!body.contains("\"temperature\""));
    assert!(body.contains(&format!("\"id\":{skill_id}")));
    cleanup(&file).await;
}

#[tokio::test]
async fn agent_set_update_persists_changes_via_api() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    let (status, body) = post_json(
        "/agent-sets",
        &headers,
        state.clone(),
        serde_json::json!({ "name": "ops", "agents": [agent_json("dev")] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let set_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    // Правка состава: новое имя и другой агент.
    let (status, body) = patch_json(
        &format!("/agent-sets/{set_id}"),
        &headers,
        state.clone(),
        serde_json::json!({ "name": "ops2", "agents": [agent_json("api")] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("ops2"));
    assert!(body.contains("api"));
    // После обновления страницы состав прежний — изменения сохранились.
    let (status, body) = get(&format!("/agent-sets/{set_id}"), &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("ops2"));
    assert!(body.contains("api"));
    assert!(!body.contains("dev"));
    cleanup(&file).await;
}
