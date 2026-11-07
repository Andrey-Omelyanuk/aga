//! Тесты HTTP API: запросы через роутер (`tower::ServiceExt::oneshot`) на
//! временной БД. Разбиты по тем же доменам, что и хендлеры; здесь — общие
//! помощники (состояние, токены SSO, запросы, чат и сообщения).

use super::*;
use axum::body::Body;
use http::{Request, StatusCode};
use tower::ServiceExt;

mod agent_sets;
mod capabilities;
mod chat;
mod error;
mod llm;
mod mcp;
mod projects;
mod router;
mod sso;
mod workstations;

async fn test_state(sso: bool) -> (AppState, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("aga_srv_test_{}.db", uuid::Uuid::new_v4()));
    let trace_store = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let chat_store = crate::chat::ChatStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let config = Config {
        sso: None,
        centrifuge: None,
    };
    let cluster = Cluster {
        backend: crate::cluster::Backend::K8s,
        kubectl: "kubectl".into(),
        namespace: "default".into(),
        template: "/nonexistent.yaml".into(),
        image: "img".into(),
        wait_timeout_secs: 1,
    };
    let centrifuge = CentrifugeClient::from_config(&crate::config::CentrifugeConfig {
        api_url: "http://centrifugo:8000".into(),
        api_key: "key".into(),
        secret: "secret".into(),
        channel: "common".into(),
    });
    let sso_verifier = Arc::new(RwLock::new(if sso {
        Some(auth::JwtVerifier::from_jwks_json(auth::TEST_JWKS).unwrap())
    } else {
        None
    }));
    let state = AppState {
        config,
        trace_store,
        chat_store,
        cluster,
        centrifuge,
        sso_verifier,
        front_url: "http://dev.localhost".into(),
    };
    (state, path)
}

fn auth_headers(sub: &str, roles: &[&str]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        format!("Bearer {}", auth::test_sign_token(sub, roles))
            .parse()
            .unwrap(),
    );
    headers
}

async fn get(uri: &str, headers: &HeaderMap, state: AppState) -> (StatusCode, String) {
    let router = create_router(state);
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(auth) = headers.get("authorization") {
        builder = builder.header("authorization", auth);
    }
    let resp = router
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn delete_json(uri: &str, headers: &HeaderMap, state: AppState) -> (StatusCode, String) {
    let router = create_router(state);
    let mut builder = Request::builder().method("DELETE").uri(uri);
    if let Some(auth) = headers.get("authorization") {
        builder = builder.header("authorization", auth);
    }
    let resp = router
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn post_json(
    uri: &str,
    headers: &HeaderMap,
    state: AppState,
    body: serde_json::Value,
) -> (StatusCode, String) {
    let router = create_router(state);
    let mut builder = Request::builder().method("POST").uri(uri);
    builder = builder.header("content-type", "application/json");
    if let Some(auth) = headers.get("authorization") {
        builder = builder.header("authorization", auth);
    }
    let resp = router
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn cleanup(path: &std::path::Path) {
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(path);
}

async fn patch_json(
    uri: &str,
    headers: &HeaderMap,
    state: AppState,
    body: serde_json::Value,
) -> (StatusCode, String) {
    let router = create_router(state);
    let mut builder = Request::builder().method("PATCH").uri(uri);
    builder = builder.header("content-type", "application/json");
    if let Some(auth) = headers.get("authorization") {
        builder = builder.header("authorization", auth);
    }
    let resp = router
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

fn agent_json(name: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "description": format!("Правила {name}"),
        "tools": ["git", "make"],
        "max_iterations": 3,
        "llm_id": null,
        "parent": null,
        "skills": []
    })
}

fn json_get(body: &str, path: &[&str]) -> Option<serde_json::Value> {
    let mut v = serde_json::from_str::<serde_json::Value>(body).ok()?;
    for key in path {
        v = v.get(*key)?.clone();
    }
    Some(v)
}

/// Создать обычный чат через API и вернуть его id.
async fn create_chat(headers: &HeaderMap, state: &AppState) -> i64 {
    let (_, body) = post_json(
        "/chats",
        headers,
        state.clone(),
        serde_json::json!({"title": "s"}),
    )
    .await;
    json_get(&body, &["id"]).unwrap().as_i64().unwrap()
}

/// Отправить сообщение в чат и вернуть тело ответа.
async fn send_chat_message(
    chat_id: i64,
    body: &str,
    headers: &HeaderMap,
    state: &AppState,
) -> (StatusCode, String) {
    post_json(
        &format!("/chats/{chat_id}/messages"),
        headers,
        state.clone(),
        serde_json::json!({"body": body}),
    )
    .await
}
