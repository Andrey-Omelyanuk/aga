//! Воркстейшны: права на управление, содержимое и изменения проекта.

use super::*;

#[tokio::test]
async fn participant_cannot_create_workstation() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    let (status, _) = post_json(
        "/workstations",
        &headers,
        state.clone(),
        serde_json::json!({"name": "w1"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    cleanup(&file).await;
}

#[tokio::test]
async fn workstations_listed_with_state() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    // Админ (внешний) поднял воркстейшн — участник видит его и состояние.
    state
        .chat_store
        .create_workstation("ws-1", None)
        .await
        .unwrap();
    let (status, body) = get("/workstations", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let workstations: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
    assert_eq!(workstations.len(), 1);
    assert_eq!(workstations[0]["name"], "ws-1");
    assert!(workstations[0]["state"].is_string());
    cleanup(&file).await;
}

#[tokio::test]
async fn releasing_workstation_forbidden_for_participant() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    // Отпускание воркстейшна — процедура админа.
    let (status, _) = post_json(
        "/workstations/1/release",
        &headers,
        state.clone(),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    cleanup(&file).await;
}

#[tokio::test]
async fn marking_workstation_down_forbidden_for_participant() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    // Отметка падения станции — процедура админа.
    let (status, _) = post_json(
        "/workstations/1/down",
        &headers,
        state.clone(),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    cleanup(&file).await;
}

#[tokio::test]
async fn workstation_tree_requires_authentication() {
    let (state, file) = test_state(true).await;
    let (status, _) = get("/workstations/1/tree", &HeaderMap::new(), state.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    cleanup(&file).await;
}

#[tokio::test]
async fn workstation_file_requires_authentication() {
    let (state, file) = test_state(true).await;
    let (status, _) = get(
        "/workstations/1/file?path=README.md",
        &HeaderMap::new(),
        state.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    cleanup(&file).await;
}

#[tokio::test]
async fn participant_browses_any_workstation_content() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    state
        .chat_store
        .create_workstation("ws-1", None)
        .await
        .unwrap();
    // Участник проходит проверку доступа к содержимому любого воркстейшна.
    // Исполнение команды в тестовом окружении может упасть по-разному
    // (нет kubectl/docker — 500; kubectl есть, но пода нет — 404), поэтому
    // проверяем не код ошибки, а то, что это не 401/403: видимость пройдена.
    let (status, _) = get("/workstations/1/tree", &headers, state.clone()).await;
    assert_ne!(status, StatusCode::UNAUTHORIZED);
    assert_ne!(status, StatusCode::FORBIDDEN);
    cleanup(&file).await;
}

#[tokio::test]
async fn missing_workstation_content_returns_not_found() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    let (status, _) = get("/workstations/99/tree", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    cleanup(&file).await;
}

#[tokio::test]
async fn no_write_route_for_project_files() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    // Просмотр только для чтения: запись файлов проекта не существует.
    let (status, _) = post_json(
        "/workstations/1/file",
        &headers,
        state.clone(),
        serde_json::json!({"path": "README.md", "content": "x"}),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    cleanup(&file).await;
}

#[tokio::test]
async fn workstation_changes_requires_authentication() {
    let (state, file) = test_state(true).await;
    let (status, _) = get("/workstations/1/changes", &HeaderMap::new(), state.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    cleanup(&file).await;
}

#[tokio::test]
async fn missing_workstation_changes_returns_not_found() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    let (status, _) = get("/workstations/99/changes", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    cleanup(&file).await;
}

#[tokio::test]
async fn participant_browses_any_workstation_changes() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    state
        .chat_store
        .create_workstation("ws-1", None)
        .await
        .unwrap();
    // Видимость как у содержимого проекта: участник проходит проверку
    // доступа к Changes любого воркстейшна. Исполнение git в тестовом
    // окружении может упасть (нет kubectl/docker) — проверяем, что это
    // не 401/403: видимость пройдена.
    let (status, _) = get("/workstations/1/changes", &headers, state.clone()).await;
    assert_ne!(status, StatusCode::UNAUTHORIZED);
    assert_ne!(status, StatusCode::FORBIDDEN);
    cleanup(&file).await;
}

#[tokio::test]
async fn no_write_route_for_changes() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    // Changes только показывает изменения: коммитить и пушить нельзя — на
    // ручке нет write-методов.
    let (status, _) = post_json(
        "/workstations/1/changes",
        &headers,
        state.clone(),
        serde_json::json!({"message": "push"}),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    cleanup(&file).await;
}
