//! Модель чата: пользователи, connection-JWT, нити, события Centrifugo.

use super::*;

#[tokio::test]
async fn personnel_listed_from_sso_but_not_editable() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    let (status, body) = get("/users", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    // Участник виден в списке.
    assert!(body.contains("alice"));
    // Создания/редактирования внутри aga нет: POST /users не существует.
    let (status, _) = post_json(
        "/users",
        &headers,
        state.clone(),
        serde_json::json!({"name": "mallory"}),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    cleanup(&file).await;
}

#[tokio::test]
async fn authenticated_user_gets_connection_jwt() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    let (status, body) = get("/connection-jwt/", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    let token = value["token"].as_str().unwrap();
    // Токен подписан тем же HMAC-секретом, что знает Centrifugo.
    let data = jsonwebtoken::decode::<serde_json::Value>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(b"secret"),
        &jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256),
    )
    .unwrap();
    // sub — id участника (chat_users.id), а не его SSO-субъект.
    let user_id: i64 = data.claims["sub"].as_str().unwrap().parse().unwrap();
    assert!(user_id > 0);
    assert_eq!(data.claims["channels"][0], "common");
    cleanup(&file).await;
}

#[tokio::test]
async fn connection_jwt_requires_authentication() {
    let (state, file) = test_state(true).await;
    let headers = HeaderMap::new();
    let (status, _) = get("/connection-jwt/", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    cleanup(&file).await;
}

#[tokio::test]
async fn connection_jwt_missing_when_centrifuge_not_configured() {
    let (mut state, file) = test_state(false).await;
    // Центрифуго не настроен (клиент-заглушка) — токена нет.
    state.centrifuge = CentrifugeClient::disabled();
    let headers = HeaderMap::new();
    let (status, _) = get("/connection-jwt/", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    cleanup(&file).await;
}

#[tokio::test]
async fn current_user_returned_by_users_me() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    let (status, body) = get("/users/me", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let user: crate::chat::ChatUser = serde_json::from_str(&body).unwrap();
    assert_eq!(user.name, "alice");
    assert_eq!(user.kind, "human");
    assert!(!user.is_super_user);
    cleanup(&file).await;
}

#[tokio::test]
async fn users_me_requires_auth_with_sso() {
    let (state, file) = test_state(true).await;
    let headers = HeaderMap::new();
    let (status, _) = get("/users/me", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    cleanup(&file).await;
}

#[tokio::test]
async fn thread_created_via_api_appears_collapsed_in_parent() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let (status, body) = post_json(
        "/chats",
        &alice,
        state.clone(),
        serde_json::json!({"title": "s"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let chat_id = json_get(&body, &["id"]).unwrap().as_i64().unwrap();
    let (_, body) = post_json(
        &format!("/chats/{chat_id}/messages"),
        &alice,
        state.clone(),
        serde_json::json!({"body": "задача"}),
    )
    .await;
    let msg_id = json_get(&body, &["message", "id"])
        .unwrap()
        .as_i64()
        .unwrap();

    let (status, _) = post_json(
        &format!("/messages/{msg_id}/thread"),
        &alice,
        state.clone(),
        serde_json::json!({"title": "Обсуждение", "body": "Что падает?"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get(&format!("/chats/{chat_id}"), &alice, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    // Нить привязана к сообщению и видна в родителе свёрнутой по заголовку
    // первого сообщения; у самого чата-нити заголовка нет.
    let threads = json_get(&body, &["threads"]).unwrap();
    let threads = threads.as_array().unwrap();
    assert_eq!(threads.len(), 1);
    assert_eq!(
        threads[0]["chat"]["start_message_id"].as_i64(),
        Some(msg_id)
    );
    assert_eq!(threads[0]["chat"]["title"], serde_json::Value::Null);
    assert_eq!(
        threads[0]["messages"][0]["title"].as_str(),
        Some("Обсуждение")
    );
    cleanup(&file).await;
}

#[tokio::test]
async fn multiple_threads_from_one_message_via_api() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let (_, body) = post_json(
        "/chats",
        &alice,
        state.clone(),
        serde_json::json!({"title": "s"}),
    )
    .await;
    let chat_id = json_get(&body, &["id"]).unwrap().as_i64().unwrap();
    let (_, body) = post_json(
        &format!("/chats/{chat_id}/messages"),
        &alice,
        state.clone(),
        serde_json::json!({"body": "задача"}),
    )
    .await;
    let msg_id = json_get(&body, &["message", "id"])
        .unwrap()
        .as_i64()
        .unwrap();
    for (title, text) in [("Разбор демо", "Первое."), ("Таймеры", "Второе.")]
    {
        let (status, _) = post_json(
            &format!("/messages/{msg_id}/thread"),
            &alice,
            state.clone(),
            serde_json::json!({"title": title, "body": text}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let (_, body) = get(&format!("/chats/{chat_id}"), &alice, state.clone()).await;
    let threads = json_get(&body, &["threads"]).unwrap();
    let threads = threads.as_array().unwrap();
    assert_eq!(threads.len(), 2);
    let titles: Vec<&str> = threads
        .iter()
        .map(|t| t["messages"][0]["title"].as_str().unwrap())
        .collect();
    assert!(titles.contains(&"Разбор демо"));
    assert!(titles.contains(&"Таймеры"));
    cleanup(&file).await;
}

#[tokio::test]
async fn start_command_no_longer_creates_thread_via_api() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let (_, body) = post_json(
        "/chats",
        &alice,
        state.clone(),
        serde_json::json!({"title": "s"}),
    )
    .await;
    let chat_id = json_get(&body, &["id"]).unwrap().as_i64().unwrap();
    let (_, body) = post_json(
        &format!("/chats/{chat_id}/messages"),
        &alice,
        state.clone(),
        serde_json::json!({"body": "задача"}),
    )
    .await;
    let msg_id = json_get(&body, &["message", "id"])
        .unwrap()
        .as_i64()
        .unwrap();
    // #start больше не команда: сообщение уходит как обычное, нить не
    // создаётся — нити начинаются только действием у сообщения.
    let (status, _) = post_json(
        &format!("/chats/{chat_id}/messages"),
        &alice,
        state.clone(),
        serde_json::json!({"body": "#start Новая нить", "parent_id": msg_id}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = get(&format!("/chats/{chat_id}"), &alice, state.clone()).await;
    let threads = json_get(&body, &["threads"]).unwrap();
    assert!(threads.as_array().unwrap().is_empty());
    cleanup(&file).await;
}

#[tokio::test]
async fn message_posted_to_parent_via_api_carries_thread_link() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let (_, body) = post_json(
        "/chats",
        &alice,
        state.clone(),
        serde_json::json!({"title": "s"}),
    )
    .await;
    let chat_id = json_get(&body, &["id"]).unwrap().as_i64().unwrap();
    let (_, body) = post_json(
        &format!("/chats/{chat_id}/messages"),
        &alice,
        state.clone(),
        serde_json::json!({"body": "задача"}),
    )
    .await;
    let msg_id = json_get(&body, &["message", "id"])
        .unwrap()
        .as_i64()
        .unwrap();
    let (_, body) = post_json(
        &format!("/messages/{msg_id}/thread"),
        &alice,
        state.clone(),
        serde_json::json!({"title": "Обсуждение", "body": "Что падает?"}),
    )
    .await;
    let thread_id = json_get(&body, &["chat", "id"]).unwrap().as_i64().unwrap();
    let (_, body) = post_json(
        &format!("/chats/{thread_id}/messages"),
        &alice,
        state.clone(),
        serde_json::json!({"body": "Гоняю run-tests."}),
    )
    .await;
    let reply_id = json_get(&body, &["message", "id"])
        .unwrap()
        .as_i64()
        .unwrap();

    let (status, _) = post_json(
        &format!("/messages/{reply_id}/to-parent"),
        &alice,
        state.clone(),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // В ленте родителя появилась копия со ссылкой на сообщение-источник;
    // оригинал остался в нити.
    let (_, body) = get(&format!("/chats/{chat_id}"), &alice, state.clone()).await;
    let messages = json_get(&body, &["messages"]).unwrap();
    let messages = messages.as_array().unwrap();
    let copy = messages
        .iter()
        .find(|m| m["thread_of_id"].as_i64() == Some(msg_id))
        .expect("копия в родителе со ссылкой на источник");
    assert_eq!(copy["body"].as_str(), Some("Гоняю run-tests."));
    let (_, body) = get(&format!("/chats/{thread_id}"), &alice, state.clone()).await;
    let thread_messages = json_get(&body, &["messages"]).unwrap();
    assert!(thread_messages
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["id"].as_i64() == Some(reply_id)));
    cleanup(&file).await;
}

#[tokio::test]
async fn list_chats_via_api_shows_only_root_chats() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let (_, body) = post_json(
        "/chats",
        &alice,
        state.clone(),
        serde_json::json!({"title": "s"}),
    )
    .await;
    let chat_id = json_get(&body, &["id"]).unwrap().as_i64().unwrap();
    let (_, body) = post_json(
        &format!("/chats/{chat_id}/messages"),
        &alice,
        state.clone(),
        serde_json::json!({"body": "задача"}),
    )
    .await;
    let msg_id = json_get(&body, &["message", "id"])
        .unwrap()
        .as_i64()
        .unwrap();
    post_json(
        &format!("/messages/{msg_id}/thread"),
        &alice,
        state.clone(),
        serde_json::json!({"title": "Обсуждение", "body": "Что падает?"}),
    )
    .await;
    // В списке чатов — только корневые: нить не показывается.
    let (status, body) = get("/chats", &alice, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let chats = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    let chats = chats.as_array().unwrap();
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0]["id"].as_i64(), Some(chat_id));
    cleanup(&file).await;
}

// === События Centrifugo: чат публикует изменения в каналы чата,
// пользователя-автора и общий канал (агенты — подписчики, не часть чата).

type PublishedEvents = std::sync::Arc<tokio::sync::Mutex<Vec<(String, serde_json::Value)>>>;

/// Мок HTTP API Centrifugo: записывает публикации (канал + данные).
async fn mock_centrifugo() -> (String, tokio::task::JoinHandle<()>, PublishedEvents) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let sink: PublishedEvents = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let sink2 = sink.clone();
    let app = axum::Router::new().route(
        "/api",
        axum::routing::post(move |req: axum::extract::Request| {
            let sink = sink2.clone();
            async move {
                let body: serde_json::Value = axum::body::to_bytes(req.into_body(), usize::MAX)
                    .await
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_default();
                let channel = body["params"]["channel"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let data = body["params"]["data"].clone();
                sink.lock().await.push((channel, data));
                axum::Json(serde_json::json!({ "result": {} }))
            }
        }),
    );
    let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), handle, sink)
}

fn centrifuge_at(api_url: String) -> CentrifugeClient {
    CentrifugeClient::from_config(&crate::config::CentrifugeConfig {
        api_url,
        api_key: "key".into(),
        secret: "secret".into(),
        channel: "common".into(),
    })
}

fn published(
    events: &[(String, serde_json::Value)],
    channel: &str,
    kind: &str,
) -> Option<serde_json::Value> {
    events
        .iter()
        .find(|(ch, data)| ch == channel && data["type"] == kind)
        .map(|(_, data)| data.clone())
}

#[tokio::test]
async fn mentioning_agent_by_name_does_not_trigger_agent() {
    // Отменено историей agent-listens-user: триггер @Agent.<имя> уходит —
    // сервер только публикует события, агенты живут в отдельном процессе.
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let chat_id = create_chat(&alice, &state).await;
    let (status, _) = send_chat_message(chat_id, "@Agent.helper привет", &alice, &state).await;
    assert_eq!(status, StatusCode::OK);
    // Сообщение доставлено, но никто на него не ответил — сервер агентов
    // не запускает.
    let (status, body) = get(&format!("/chats/{chat_id}/messages"), &alice, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let messages: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
    assert_eq!(messages.len(), 1, "лишних сообщений нет");
    // «Агент-пользователь» Agent.helper не создаётся.
    let (_, users) = get("/users", &alice, state).await;
    let users: Vec<serde_json::Value> = serde_json::from_str(&users).unwrap();
    assert!(
        !users
            .iter()
            .any(|u| u["name"].as_str() == Some("Agent.helper")),
        "агент-пользователи больше не авто-создаются"
    );
    cleanup(&file).await;
}

#[tokio::test]
async fn sent_message_event_reaches_chat_channel() {
    let (mut state, file) = test_state(true).await;
    let (api_url, server, events) = mock_centrifugo().await;
    state.centrifuge = centrifuge_at(api_url);
    let alice = auth_headers("alice", &["participant"]);
    let chat_id = create_chat(&alice, &state).await;
    let (status, body) = send_chat_message(chat_id, "привет", &alice, &state).await;
    assert_eq!(status, StatusCode::OK);
    let message_id = json_get(&body, &["message", "id"])
        .unwrap()
        .as_i64()
        .unwrap();
    let evs = events.lock().await;
    let data = published(&evs, &format!("chat:{chat_id}"), "message")
        .expect("событие сообщения не пришло в канал чата");
    // Событие несёт id чата и id сообщения — подписчику этого хватает,
    // детали он дочитает сам.
    assert_eq!(data["chat_id"], chat_id);
    assert_eq!(data["message_id"], message_id);
    server.abort();
    cleanup(&file).await;
}

#[tokio::test]
async fn sent_message_event_reaches_author_user_channel() {
    let (mut state, file) = test_state(true).await;
    let (api_url, server, events) = mock_centrifugo().await;
    state.centrifuge = centrifuge_at(api_url);
    let alice = auth_headers("alice", &["participant"]);
    let chat_id = create_chat(&alice, &state).await;
    let (status, body) = send_chat_message(chat_id, "привет", &alice, &state).await;
    assert_eq!(status, StatusCode::OK);
    let message_id = json_get(&body, &["message", "id"])
        .unwrap()
        .as_i64()
        .unwrap();
    let alice_id = json_get(&body, &["message", "author_id"])
        .unwrap()
        .as_i64()
        .unwrap();
    let evs = events.lock().await;
    let data = published(&evs, &format!("user:{alice_id}"), "message")
        .expect("событие сообщения не пришло в канал автора");
    assert_eq!(data["message_id"], message_id);
    assert_eq!(data["author_id"], alice_id);
    server.abort();
    cleanup(&file).await;
}

#[tokio::test]
async fn chat_lifecycle_events_reach_common_channel() {
    let (mut state, file) = test_state(true).await;
    let (api_url, server, events) = mock_centrifugo().await;
    state.centrifuge = centrifuge_at(api_url);
    let alice = auth_headers("alice", &["participant"]);
    // Создание чата.
    let chat_id = create_chat(&alice, &state).await;
    // Сессия: проект + готовая станция (станции создаёт только суперпользователь
    // вне API — здесь фиксируем её в БД напрямую, как сид).
    let (status, body) = post_json(
        "/projects",
        &alice,
        state.clone(),
        serde_json::json!({"git_url": "https://example.com/lifecycle.git"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let project_id = json_get(&body, &["id"]).unwrap().as_i64().unwrap();
    let ws = state
        .chat_store
        .create_workstation("ws-lifecycle", None)
        .await
        .unwrap();
    state
        .chat_store
        .set_workstation_state(ws.id, "ready")
        .await
        .unwrap();
    let (status, body) = post_json(
        &format!("/workstations/{}/session", ws.id),
        &alice,
        state.clone(),
        serde_json::json!({"project_id": project_id}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = json_get(&body, &["id"]).unwrap().as_i64().unwrap();
    let (status, _) = post_json(
        &format!("/chats/{session_id}/close"),
        &alice,
        state.clone(),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let evs = events.lock().await;
    let actions: Vec<(&str, i64)> = evs
        .iter()
        .filter(|(ch, _)| ch == "common")
        .filter_map(|(_, data)| {
            let action = data["action"].as_str()?;
            Some((action, data["chat_id"].as_i64()?))
        })
        .collect();
    assert!(
        actions.contains(&("chat_created", chat_id)),
        "нет события создания чата: {actions:?}"
    );
    assert!(
        actions.contains(&("session_opened", session_id)),
        "нет события открытия сессии: {actions:?}"
    );
    assert!(
        actions.contains(&("session_closed", session_id)),
        "нет события закрытия сессии: {actions:?}"
    );
    server.abort();
    cleanup(&file).await;
}
