//! Подключения к LLM и дефолтная LLM.

use super::*;

/// Параметры генерации подключения: сохраняются и отдаются, мусор — 400.
#[tokio::test]
async fn llm_connection_generation_params_validated_and_returned() {
    let (state, file) = test_state(true).await;
    let admin = auth_headers("bob", &["admin"]);
    let base = |extra: serde_json::Value| {
        let mut v = serde_json::json!({"name": "o", "api_url": "http://l/v1", "model_name": "m"});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    };
    for bad in [
        serde_json::json!({"max_tokens": 0}),
        serde_json::json!({"max_tokens": 2_000_000}),
        serde_json::json!({"temperature": 2.5}),
        serde_json::json!({"temperature": -0.1}),
    ] {
        let (status, _) = post_json("/llms", &admin, state.clone(), base(bad.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    let (status, body) = post_json(
        "/llms",
        &admin,
        state.clone(),
        base(serde_json::json!({"max_tokens": 8192, "temperature": 0.2})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"max_tokens\":8192"), "{body}");
    assert!(body.contains("\"temperature\":0.2"), "{body}");
    cleanup(&file).await;
}

#[tokio::test]
async fn created_llm_connection_listed_via_api() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("bob", &["admin"]);
    let (status, body) = post_json(
        "/llms",
        &headers,
        state.clone(),
        serde_json::json!({
            "name": "ollama",
            "api_url": "http://llm:11434/v1",
            "api_key": "secret-key",
            "model_name": "qwen3:0.6b"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("ollama"));
    assert!(body.contains("http://llm:11434/v1"));
    assert!(body.contains("qwen3:0.6b"));
    // Ключ наружу не отдаётся — только признак, что он задан.
    assert!(!body.contains("secret-key"), "{body}");
    assert!(body.contains("\"has_api_key\":true"), "{body}");
    // Созданное подключение видно в списке любому участнику: название,
    // url и модель, но не ключ.
    let alice = auth_headers("alice", &["participant"]);
    let (status, body) = get("/llms", &alice, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("ollama"));
    assert!(body.contains("http://llm:11434/v1"));
    assert!(body.contains("qwen3:0.6b"));
    assert!(!body.contains("secret-key"), "{body}");
    assert!(!body.contains("\"api_key\""), "{body}");
    // Сам ключ сохранён — агенты ходят с ним.
    let stored = state.trace_store.list_llm_connections().await.unwrap();
    assert_eq!(stored[0].api_key.as_deref(), Some("secret-key"));
    cleanup(&file).await;
}

#[tokio::test]
async fn updated_llm_connection_changes_url_and_key_via_api() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("bob", &["admin"]);
    let (status, body) = post_json(
        "/llms",
        &headers,
        state.clone(),
        serde_json::json!({
            "name": "ollama",
            "api_url": "http://old/v1",
            "api_key": "old-key",
            "model_name": "qwen3:0.6b"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let (status, body) = patch_json(
        &format!("/llms/{id}"),
        &headers,
        state.clone(),
        serde_json::json!({
            "name": "ollama2",
            "api_url": "http://new/v1",
            "api_key": "new-key",
            "model_name": "qwen3:1b"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("http://new/v1"));
    assert!(!body.contains("new-key"), "{body}");
    assert!(body.contains("qwen3:1b"));
    assert!(!body.contains("http://old/v1"));
    let key = |state: AppState| async move {
        state
            .trace_store
            .get_llm_connection(id)
            .await
            .unwrap()
            .unwrap()
            .api_key
    };
    assert_eq!(key(state.clone()).await.as_deref(), Some("new-key"));
    // Форма правки ключа не знает: без поля (или null) ключ остаётся.
    let (status, _) = patch_json(
        &format!("/llms/{id}"),
        &headers,
        state.clone(),
        serde_json::json!({
            "name": "ollama2",
            "api_url": "http://new/v1",
            "model_name": "qwen3:1b"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(key(state.clone()).await.as_deref(), Some("new-key"));
    // Пустая строка — удалить ключ.
    let (status, body) = patch_json(
        &format!("/llms/{id}"),
        &headers,
        state.clone(),
        serde_json::json!({
            "name": "ollama2",
            "api_url": "http://new/v1",
            "api_key": "",
            "model_name": "qwen3:1b"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"has_api_key\":false"), "{body}");
    assert_eq!(key(state.clone()).await, None);
    cleanup(&file).await;
}

#[tokio::test]
async fn deleted_llm_connection_disappears_from_list_via_api() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("bob", &["admin"]);
    let (status, body) = post_json(
        "/llms",
        &headers,
        state.clone(),
        serde_json::json!({
            "name": "ollama",
            "api_url": "http://llm:11434/v1",
            "api_key": null,
            "model_name": "qwen3:0.6b"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let (status, _) = delete_json(&format!("/llms/{id}"), &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = get("/llms", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("ollama"));
    let (status, _) = delete_json(&format!("/llms/{id}"), &headers, state.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    cleanup(&file).await;
}

#[tokio::test]
async fn llm_connections_require_authentication() {
    let (state, file) = test_state(true).await;
    let headers = HeaderMap::new();
    let (status, _) = get("/llms", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = post_json(
        "/llms",
        &headers,
        state.clone(),
        serde_json::json!({
            "name": "ollama",
            "api_url": "http://llm:11434/v1",
            "api_key": null,
            "model_name": "qwen3:0.6b"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    cleanup(&file).await;
}

#[tokio::test]
async fn default_llm_set_and_cleared_via_api() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("bob", &["admin"]);
    let (status, body) = post_json(
        "/llms",
        &headers,
        state.clone(),
        serde_json::json!({
            "name": "ollama",
            "api_url": "http://llm:11434/v1",
            "api_key": null,
            "model_name": "qwen3:0.6b"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    // Выбор дефолтной LLM: подключение отмечено, снять выбор можно.
    let (status, body) = post_json(
        "/settings/llm-default",
        &headers,
        state.clone(),
        serde_json::json!({ "llm_id": id }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("ollama"));
    let (status, body) = get("/llms", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"is_default\":true"));
    let (status, body) = post_json(
        "/settings/llm-default",
        &headers,
        state.clone(),
        serde_json::json!({ "llm_id": null }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "null");
    let (status, body) = get("/llms", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"is_default\":false"));
    // Несуществующее подключение дефолтным не становится.
    let (status, _) = post_json(
        "/settings/llm-default",
        &headers,
        state.clone(),
        serde_json::json!({ "llm_id": 9999 }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    cleanup(&file).await;
}

#[tokio::test]
async fn default_llm_requires_authentication() {
    let (state, file) = test_state(true).await;
    let headers = HeaderMap::new();
    let (status, _) = post_json(
        "/settings/llm-default",
        &headers,
        state.clone(),
        serde_json::json!({ "llm_id": null }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    cleanup(&file).await;
}
