//! Роутер целиком: гейт токена, аноним без SSO, права админа на настройки
//! платформы, удалённые ручки.

use super::*;

#[tokio::test]
async fn invalid_token_rejected_by_api() {
    let (state, file) = test_state(true).await;
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        "Bearer not.a.jwt".parse().unwrap(),
    );
    let (status, _) = get("/projects", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    cleanup(&file).await;
}

#[tokio::test]
async fn anonymous_superuser_works_without_sso() {
    let (state, file) = test_state(false).await;
    let headers = HeaderMap::new();
    // Без SSO списки работают — ручки не требуют токена.
    let (status, _) = get("/projects", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    cleanup(&file).await;
}

/// Настройки платформы меняет только админ: участник читает список
/// подключений и MCP-серверов, но не создаёт, не правит и не удаляет их,
/// не меняет дефолтную LLM и не удаляет проекты.
#[tokio::test]
async fn participant_cannot_change_platform_settings() {
    let (state, file) = test_state(true).await;
    let admin = auth_headers("bob", &["admin"]);
    let alice = auth_headers("alice", &["participant"]);
    let llm = serde_json::json!({
        "name": "ollama", "api_url": "http://llm/v1", "api_key": "k", "model_name": "m"
    });
    let mcp = serde_json::json!({"name": "gh", "transport": "http", "url": "http://mcp"});
    let (_, body) = post_json("/llms", &admin, state.clone(), llm.clone()).await;
    let llm_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let (_, body) = post_json("/mcp-servers", &admin, state.clone(), mcp.clone()).await;
    let mcp_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let project_id = state
        .trace_store
        .upsert_project("https://example.com/a.git")
        .await
        .unwrap();

    let forbidden = [
        post_json("/llms", &alice, state.clone(), llm.clone())
            .await
            .0,
        patch_json(
            &format!("/llms/{llm_id}"),
            &alice,
            state.clone(),
            llm.clone(),
        )
        .await
        .0,
        delete_json(&format!("/llms/{llm_id}"), &alice, state.clone())
            .await
            .0,
        post_json(
            "/settings/llm-default",
            &alice,
            state.clone(),
            serde_json::json!({"llm_id": llm_id}),
        )
        .await
        .0,
        post_json("/mcp-servers", &alice, state.clone(), mcp.clone())
            .await
            .0,
        patch_json(
            &format!("/mcp-servers/{mcp_id}"),
            &alice,
            state.clone(),
            mcp.clone(),
        )
        .await
        .0,
        delete_json(&format!("/mcp-servers/{mcp_id}"), &alice, state.clone())
            .await
            .0,
        delete_json(&format!("/projects/{project_id}"), &alice, state.clone())
            .await
            .0,
    ];
    assert!(
        forbidden.iter().all(|s| *s == StatusCode::FORBIDDEN),
        "{forbidden:?}"
    );
    // Читать участнику можно — без ключей.
    let (status, body) = get("/mcp-servers", &alice, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"has_api_key\":false"), "{body}");
    let (status, _) = get("/llms", &alice, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    // Админ удаляет проект.
    let (status, _) = delete_json(&format!("/projects/{project_id}"), &admin, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    cleanup(&file).await;
}

/// Гейт роутера: без токена при включённом SSO закрыто всё, кроме входа и
/// пробы живости; неизвестный путь — 404, а не 401.
#[tokio::test]
async fn every_route_except_auth_and_healthz_requires_token() {
    let (state, file) = test_state(true).await;
    let none = HeaderMap::new();
    for uri in [
        "/trace/some-task",
        "/messages/1/artifacts",
        "/projects",
        "/llms",
        "/mcp-servers",
        "/workstations",
        "/settings/ssh-key",
    ] {
        let (status, _) = get(uri, &none, state.clone()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
    }
    let (status, body) = get("/healthz", &none, state.clone()).await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "ok"));
    let (status, _) = get("/no-such-route", &none, state.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // С токеном трасса и артефакты доступны (здесь — 404: их нет).
    let alice = auth_headers("alice", &["participant"]);
    let (status, _) = get("/trace/some-task", &alice, state.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get("/messages/1/artifacts", &alice, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    cleanup(&file).await;
}

#[tokio::test]
async fn role_endpoints_removed() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    // Глобальные роли и ручная настройка ролей проекта исчезли.
    let (status, _) = get("/roles", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get("/projects/1/roles", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = post_json(
        "/projects/1/roles",
        &headers,
        state.clone(),
        serde_json::json!({ "active_roles": ["dev"] }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    cleanup(&file).await;
}
