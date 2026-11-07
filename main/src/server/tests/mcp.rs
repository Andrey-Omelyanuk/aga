//! Каталог MCP-серверов и проверка инструментов.

use super::*;

#[tokio::test]
async fn mcp_servers_catalog_and_agent_links_via_api() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("bob", &["admin"]);
    let (status, body) = post_json(
        "/mcp-servers",
        &headers,
        state.clone(),
        serde_json::json!({"name": "gh", "transport": "http", "url": "http://mcp/gh", "api_key": "k"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    // Имя уникально; транспорт и его обязательное поле проверяются.
    for (spec, code) in [
        (
            serde_json::json!({"name": "gh", "transport": "http", "url": "http://x"}),
            StatusCode::CONFLICT,
        ),
        (
            serde_json::json!({"name": "fs", "transport": "stdio"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            serde_json::json!({"name": "fs", "transport": "ws", "url": "x"}),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let (status, _) = post_json("/mcp-servers", &headers, state.clone(), spec).await;
        assert_eq!(status, code);
    }
    let mut agent = agent_json("dev");
    agent["mcp"] = serde_json::json!(["gh", "missing"]);
    let (status, body) = post_json(
        "/agent-sets",
        &headers,
        state.clone(),
        serde_json::json!({ "name": "ops", "agents": [agent] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let set_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let agent_mcp = |body: &str| {
        serde_json::from_str::<serde_json::Value>(body).unwrap()["agents"][0]["mcp"].clone()
    };
    let (_, body) = get(&format!("/agent-sets/{set_id}"), &headers, state.clone()).await;
    // Неизвестное имя пропущено.
    assert_eq!(agent_mcp(&body), serde_json::json!(["gh"]));
    // Переименование сервера не рвёт привязку агента.
    let (status, _) = patch_json(
        &format!("/mcp-servers/{id}"),
        &headers,
        state.clone(),
        serde_json::json!({"name": "github", "transport": "http", "url": "http://mcp/gh"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = get(&format!("/agent-sets/{set_id}"), &headers, state.clone()).await;
    assert_eq!(agent_mcp(&body), serde_json::json!(["github"]));
    // Удаление сервера отвязывает его от агентов.
    let (status, _) = delete_json(&format!("/mcp-servers/{id}"), &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = get(&format!("/agent-sets/{set_id}"), &headers, state.clone()).await;
    assert_eq!(agent_mcp(&body), serde_json::json!([]));
    let (_, body) = get("/mcp-servers", &headers, state.clone()).await;
    assert_eq!(body, "[]");
    cleanup(&file).await;
}

#[tokio::test]
async fn mcp_http_server_tools_checked_via_api() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("bob", &["admin"]);
    let (url, server, _) = crate::mcp::tests::http_server().await;
    let (_, body) = post_json(
        "/mcp-servers",
        &headers,
        state.clone(),
        serde_json::json!({"name": "h", "transport": "http", "url": url}),
    )
    .await;
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let (status, body) = get(&format!("/mcp-servers/{id}/tools"), &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"echo\""), "{body}");
    // stdio-сервер отсюда не проверяется.
    let (_, body) = post_json(
        "/mcp-servers",
        &headers,
        state.clone(),
        serde_json::json!({"name": "fs", "transport": "stdio", "command": "mcp-fs"}),
    )
    .await;
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let (status, _) = get(&format!("/mcp-servers/{id}/tools"), &headers, state.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Недоступный http-сервер — 502 с текстом ошибки.
    let (_, body) = post_json(
        "/mcp-servers",
        &headers,
        state.clone(),
        serde_json::json!({"name": "down", "transport": "http", "url": "http://127.0.0.1:1/mcp"}),
    )
    .await;
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let (status, body) = get(&format!("/mcp-servers/{id}/tools"), &headers, state.clone()).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(body.contains("HTTP"), "{body}");
    server.abort();
    cleanup(&file).await;
}
