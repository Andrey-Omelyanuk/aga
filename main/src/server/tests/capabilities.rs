//! Каталог способностей: скиллы, сокращения и скрытая часть сообщения, история.

use super::*;

#[tokio::test]
async fn capability_renamed_and_deleted_via_api() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
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
    // Переименование через PATCH сохраняет содержимое.
    let (status, body) = patch_json(
        &format!("/skills/{skill_id}"),
        &headers,
        state.clone(),
        serde_json::json!({ "name": "review2" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("review2"));
    assert!(body.contains("Проверять диф"));
    // Правка содержимого через PATCH делает его текущим.
    let (status, body) = patch_json(
        &format!("/skills/{skill_id}"),
        &headers,
        state.clone(),
        serde_json::json!({ "content": "Проверять диф и тесты" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Проверять диф и тесты"));
    let (status, body) = get("/skills", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("review2"));
    assert!(body.contains("Проверять диф и тесты"));
    assert!(!body.contains("пinned_version"));
    // Несуществующая способность — 404.
    let (status, _) = patch_json(
        "/skills/999",
        &headers,
        state.clone(),
        serde_json::json!({ "name": "x" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Удаление убирает из активного списка; повторное — 404.
    let (status, _) = delete_json(&format!("/skills/{skill_id}"), &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = get("/skills", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("review2"));
    let (status, _) = delete_json(&format!("/skills/{skill_id}"), &headers, state.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Удалённая запись остаётся в списке «Удалённые» (?deleted=1).
    let (status, body) = get("/skills?deleted=1", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("review2"));
    cleanup(&file).await;
}

#[tokio::test]
async fn capability_history_shows_who_when_and_what_via_api() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    let (status, body) = post_json(
        "/skills",
        &headers,
        state.clone(),
        serde_json::json!({ "name": "review", "content": "v1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let skill_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    patch_json(
        &format!("/skills/{skill_id}"),
        &headers,
        state.clone(),
        serde_json::json!({ "content": "v2" }),
    )
    .await;
    patch_json(
        &format!("/skills/{skill_id}"),
        &headers,
        state.clone(),
        serde_json::json!({ "name": "review2" }),
    )
    .await;
    delete_json(&format!("/skills/{skill_id}"), &headers, state.clone()).await;
    // История по одной записи: кто, когда и что сделал, по порядку.
    let (status, body) = get(
        &format!("/skills/{skill_id}/history"),
        &headers,
        state.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let entries: serde_json::Value = serde_json::from_str(&body).unwrap();
    let actions: Vec<&str> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert_eq!(actions, vec!["create", "update", "rename", "delete"]);
    // Автор каждой правки — участник, сделавший её (alice).
    let actor_names: Vec<&str> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["actor_name"].as_str().unwrap())
        .collect();
    assert_eq!(actor_names, vec!["alice", "alice", "alice", "alice"]);
    // История несуществующей записи — 404.
    let (status, _) = get("/skills/999/history", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    cleanup(&file).await;
}

#[tokio::test]
async fn capability_edit_skips_unchanged_name_and_content_and_conflicts_return_409() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    let (status, body) = post_json(
        "/skills",
        &headers,
        state.clone(),
        serde_json::json!({ "name": "review", "content": "v1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let skill_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    // Фронт всегда шлёт имя: правка содержимого с тем же именем не должна
    // писать в историю лишнее «переименовал».
    let (status, _) = patch_json(
        &format!("/skills/{skill_id}"),
        &headers,
        state.clone(),
        serde_json::json!({ "name": "review", "content": "v2" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = get(
        &format!("/skills/{skill_id}/history"),
        &headers,
        state.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let entries: serde_json::Value = serde_json::from_str(&body).unwrap();
    let actions: Vec<&str> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert_eq!(actions, vec!["create", "update"]);
    // И наоборот: переименование с тем же содержимым — только «переименовал»,
    // без пустого «изменил содержимое».
    let (status, _) = patch_json(
        &format!("/skills/{skill_id}"),
        &headers,
        state.clone(),
        serde_json::json!({ "name": "review-2", "content": "v2" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = get(
        &format!("/skills/{skill_id}/history"),
        &headers,
        state.clone(),
    )
    .await;
    let entries: serde_json::Value = serde_json::from_str(&body).unwrap();
    let actions: Vec<&str> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert_eq!(actions, vec!["create", "update", "rename"]);
    let (status, _) = patch_json(
        &format!("/skills/{skill_id}"),
        &headers,
        state.clone(),
        serde_json::json!({ "name": "review" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Переименование в занятое имя — 409, а не 500.
    let (status, _) = post_json(
        "/skills",
        &headers,
        state.clone(),
        serde_json::json!({ "name": "taken", "content": "x" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = patch_json(
        &format!("/skills/{skill_id}"),
        &headers,
        state.clone(),
        serde_json::json!({ "name": "taken" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    // Имя не поменялось — содержимое на месте.
    let (status, body) = get(&format!("/skills/{skill_id}"), &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"name\":\"review\""));
    assert!(body.contains("v2"));
    // Создание с занятым именем — тоже 409.
    let (status, _) = post_json(
        "/skills",
        &headers,
        state.clone(),
        serde_json::json!({ "name": "taken", "content": "y" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    cleanup(&file).await;
}

// === Сокращения и скрытая часть сообщения ===

#[tokio::test]
async fn shortcut_token_adds_text_to_message_hidden() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let (status, _) = post_json(
        "/shortcuts",
        &alice,
        state.clone(),
        serde_json::json!({"name": "review", "content": "Проверять диф"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let chat_id = create_chat(&alice, &state).await;
    let (status, body) =
        send_chat_message(chat_id, "Сделай /review пожалуйста", &alice, &state).await;
    assert_eq!(status, StatusCode::OK);
    // Видимый текст не меняется, скрытая часть — из сокращения.
    assert_eq!(
        json_get(&body, &["message", "body"]).unwrap().as_str(),
        Some("Сделай /review пожалуйста")
    );
    assert_eq!(
        json_get(&body, &["message", "hidden"]).unwrap().as_str(),
        Some("Проверять диф")
    );
    cleanup(&file).await;
}

#[tokio::test]
async fn unknown_shortcut_leaves_message_hidden_unchanged() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let chat_id = create_chat(&alice, &state).await;
    let (status, body) = send_chat_message(chat_id, "/nope вопрос", &alice, &state).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json_get(&body, &["message", "hidden"]).unwrap().as_str(),
        Some("")
    );
    cleanup(&file).await;
}

#[tokio::test]
async fn repeated_shortcut_added_once() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    post_json(
        "/shortcuts",
        &alice,
        state.clone(),
        serde_json::json!({"name": "review", "content": "Проверять диф"}),
    )
    .await;
    let chat_id = create_chat(&alice, &state).await;
    let (_, body) = send_chat_message(chat_id, "/review и ещё /review", &alice, &state).await;
    assert_eq!(
        json_get(&body, &["message", "hidden"]).unwrap().as_str(),
        Some("Проверять диф")
    );
    cleanup(&file).await;
}

#[tokio::test]
async fn several_shortcuts_added_to_hidden() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    for (name, content) in [("first", "Первый текст"), ("second", "Второй текст")]
    {
        post_json(
            "/shortcuts",
            &alice,
            state.clone(),
            serde_json::json!({"name": name, "content": content}),
        )
        .await;
    }
    let chat_id = create_chat(&alice, &state).await;
    let (_, body) = send_chat_message(chat_id, "/first и /second", &alice, &state).await;
    // Оба текста в скрытой части, порядок — по появлению в сообщении.
    assert_eq!(
        json_get(&body, &["message", "hidden"]).unwrap().as_str(),
        Some("Первый текст\nВторой текст")
    );
    cleanup(&file).await;
}

#[tokio::test]
async fn shortcut_in_thread_first_message_adds_hidden() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    post_json(
        "/shortcuts",
        &alice,
        state.clone(),
        serde_json::json!({"name": "review", "content": "Проверять диф"}),
    )
    .await;
    let chat_id = create_chat(&alice, &state).await;
    let (_, body) = send_chat_message(chat_id, "задача", &alice, &state).await;
    let msg_id = json_get(&body, &["message", "id"])
        .unwrap()
        .as_i64()
        .unwrap();
    // Первое сообщение нити — тоже обычное сообщение: сокращение работает.
    let (status, body) = post_json(
        &format!("/messages/{msg_id}/thread"),
        &alice,
        state.clone(),
        serde_json::json!({"title": "Обсуждение", "body": "Смотри /review"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json_get(&body, &["message", "hidden"]).unwrap().as_str(),
        Some("Проверять диф")
    );
    cleanup(&file).await;
}

#[tokio::test]
async fn hidden_not_in_agent_context() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    post_json(
        "/shortcuts",
        &alice,
        state.clone(),
        serde_json::json!({"name": "secret", "content": "СЕКРЕТНАЯ ЗАМЕТКА"}),
    )
    .await;
    let chat_id = create_chat(&alice, &state).await;
    send_chat_message(chat_id, "/secret вопрос", &alice, &state).await;
    // В контекст агентов идёт только тело: скрытой заметки там нет.
    let trigger = state
        .chat_store
        .list_messages(chat_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    // Глазами другого участника (агента): чужое сообщение — только видимый текст.
    let context = crate::context::build_context(&state.chat_store, chat_id, 0, trigger.id, true)
        .await
        .unwrap();
    let text: String = context.iter().map(|m| m.content.clone()).collect();
    assert!(text.contains("/secret вопрос"));
    assert!(!text.contains("СЕКРЕТНАЯ ЗАМЕТКА"));
    cleanup(&file).await;
}

#[tokio::test]
async fn editing_or_deleting_shortcut_does_not_change_sent_message_hidden() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let (_, body) = post_json(
        "/shortcuts",
        &alice,
        state.clone(),
        serde_json::json!({"name": "review", "content": "Версия 1"}),
    )
    .await;
    let shortcut_id = json_get(&body, &["id"]).unwrap().as_i64().unwrap();
    let chat_id = create_chat(&alice, &state).await;
    let (_, body) = send_chat_message(chat_id, "/review", &alice, &state).await;
    let msg_id = json_get(&body, &["message", "id"])
        .unwrap()
        .as_i64()
        .unwrap();
    // Правка и удаление сокращения не трогают уже отправленное сообщение:
    // скрытая часть — снимок на момент отправки.
    patch_json(
        &format!("/shortcuts/{shortcut_id}"),
        &alice,
        state.clone(),
        serde_json::json!({"content": "Версия 2"}),
    )
    .await;
    delete_json(&format!("/shortcuts/{shortcut_id}"), &alice, state.clone()).await;
    let (_, body) = get(&format!("/chats/{chat_id}"), &alice, state.clone()).await;
    let messages = json_get(&body, &["messages"]).unwrap();
    let msg = messages
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"].as_i64() == Some(msg_id))
        .unwrap();
    assert_eq!(msg["hidden"].as_str(), Some("Версия 1"));
    cleanup(&file).await;
}

#[tokio::test]
async fn sent_message_hidden_cannot_be_changed() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let chat_id = create_chat(&alice, &state).await;
    // Скрытую часть нельзя задать вводом: присланное поле игнорируется,
    // она собирается только из сокращений.
    let (_, body) = post_json(
        &format!("/chats/{chat_id}/messages"),
        &alice,
        state.clone(),
        serde_json::json!({"body": "текст", "hidden": "подмена"}),
    )
    .await;
    assert_eq!(
        json_get(&body, &["message", "hidden"]).unwrap().as_str(),
        Some("")
    );
    cleanup(&file).await;
}

#[tokio::test]
async fn shortcut_name_allows_any_alphabet_without_spaces() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    // Имя может быть на любом алфавите.
    let (status, _) = post_json(
        "/shortcuts",
        &alice,
        state.clone(),
        serde_json::json!({"name": "проверка", "content": "Текст"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Пробел в имени недопустим: слово `/имя` не набрать.
    let (status, _) = post_json(
        "/shortcuts",
        &alice,
        state.clone(),
        serde_json::json!({"name": "my shortcut", "content": "Текст"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Переименование в имя с пробелом тоже отклоняется.
    let (_, body) = post_json(
        "/shortcuts",
        &alice,
        state.clone(),
        serde_json::json!({"name": "ok", "content": "Текст"}),
    )
    .await;
    let id = json_get(&body, &["id"]).unwrap().as_i64().unwrap();
    let (status, _) = patch_json(
        &format!("/shortcuts/{id}"),
        &alice,
        state.clone(),
        serde_json::json!({"name": "bad name"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    cleanup(&file).await;
}

#[tokio::test]
async fn shortcut_occupied_name_rejected() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let (status, _) = post_json(
        "/shortcuts",
        &alice,
        state.clone(),
        serde_json::json!({"name": "dup", "content": "a"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = post_json(
        "/shortcuts",
        &alice,
        state.clone(),
        serde_json::json!({"name": "dup", "content": "b"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    cleanup(&file).await;
}

#[tokio::test]
async fn shortcut_changes_written_to_history() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let (_, body) = post_json(
        "/shortcuts",
        &alice,
        state.clone(),
        serde_json::json!({"name": "review", "content": "v1"}),
    )
    .await;
    let id = json_get(&body, &["id"]).unwrap().as_i64().unwrap();
    patch_json(
        &format!("/shortcuts/{id}"),
        &alice,
        state.clone(),
        serde_json::json!({"content": "v2"}),
    )
    .await;
    patch_json(
        &format!("/shortcuts/{id}"),
        &alice,
        state.clone(),
        serde_json::json!({"name": "review2"}),
    )
    .await;
    delete_json(&format!("/shortcuts/{id}"), &alice, state.clone()).await;
    let (status, body) = get(&format!("/shortcuts/{id}/history"), &alice, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let entries: serde_json::Value = serde_json::from_str(&body).unwrap();
    let actions: Vec<&str> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert_eq!(actions, vec!["create", "update", "rename", "delete"]);
    cleanup(&file).await;
}

#[tokio::test]
async fn deleted_shortcut_stays_with_history() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let (_, body) = post_json(
        "/shortcuts",
        &alice,
        state.clone(),
        serde_json::json!({"name": "review", "content": "v1"}),
    )
    .await;
    let id = json_get(&body, &["id"]).unwrap().as_i64().unwrap();
    delete_json(&format!("/shortcuts/{id}"), &alice, state.clone()).await;
    // Удалённая запись видна в списке «Удалённые» и её историю можно открыть.
    let (status, body) = get("/shortcuts?deleted=1", &alice, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("review"));
    let (status, _) = get(&format!("/shortcuts/{id}/history"), &alice, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    cleanup(&file).await;
}

#[tokio::test]
async fn any_participant_can_edit_shortcuts_and_see_hidden() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let bob = auth_headers("bob", &["participant"]);
    // Сокращение создал alice — перечень общий, его видит и правит bob.
    post_json(
        "/shortcuts",
        &alice,
        state.clone(),
        serde_json::json!({"name": "review", "content": "Проверять диф"}),
    )
    .await;
    let (status, body) = get("/shortcuts", &bob, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("review"));
    let (_, body) =
        send_chat_message(create_chat(&bob, &state).await, "/review", &bob, &state).await;
    // Скрытую часть видит любой участник.
    assert_eq!(
        json_get(&body, &["message", "hidden"]).unwrap().as_str(),
        Some("Проверять диф")
    );
    cleanup(&file).await;
}
