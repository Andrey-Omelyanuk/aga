//! Модель чата: пользователи (`/users`, `/users/me`), connection-JWT
//! Centrifugo, чаты, сообщения и команды, нити, шаринг, артефакты.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};

use super::{can_read, can_write, current_user, publish_lifecycle, AppError, AppState};
use crate::chat::{parse_command, Chat, ChatCommand, Message, SessionError};

// === Модель чата ===

pub(super) async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::chat::ChatUser>>, AppError> {
    current_user(&state, &headers).await?;
    Ok(Json(state.chat_store.list_users().await?))
}

pub(super) async fn get_user(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::chat::ChatUser>, AppError> {
    current_user(&state, &headers).await?;
    let user = state
        .chat_store
        .get_user(id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(user))
}

/// Текущий пользователь (по токену/куке). Веб-клиент берёт его из `/users/me`
/// для отображения в шапке; он же — проба доступа (401 = нужен вход).
pub(super) async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::chat::ChatUser>, AppError> {
    let user_id = current_user(&state, &headers).await?;
    let user = state
        .chat_store
        .get_user(user_id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(user))
}

#[derive(Serialize)]
pub(super) struct ConnectionJwtResponse {
    token: String,
}

/// Connection-JWT для веб-клиента Centrifuge. Только для аутентифицированных
/// (current_user → 401 без токена); Centrifugo не настроен — 404, клиент
/// деградирует молча.
pub(super) async fn connection_jwt(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ConnectionJwtResponse>, AppError> {
    let user_id = current_user(&state, &headers).await?;
    if !state.centrifuge.is_configured() {
        return Err(StatusCode::NOT_FOUND.into());
    }
    let token = state.centrifuge.connection_jwt(user_id)?;
    Ok(Json(ConnectionJwtResponse { token }))
}

#[derive(Deserialize)]
pub struct CreateChatRequest {
    #[serde(default)]
    pub title: Option<String>,
}

pub(super) async fn create_chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateChatRequest>,
) -> Result<Json<Chat>, AppError> {
    let user_id = current_user(&state, &headers).await?;
    let chat = state
        .chat_store
        .create_chat(None, payload.title.as_deref(), user_id)
        .await?;
    publish_lifecycle(&state, "chat_created", chat.id).await;
    Ok(Json(chat))
}

pub(super) async fn list_chats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Chat>>, AppError> {
    let user_id = current_user(&state, &headers).await?;
    Ok(Json(state.chat_store.list_chats_for_user(user_id).await?))
}

pub(super) async fn get_chat(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = current_user(&state, &headers).await?;
    if !can_read(&state, id, user_id).await {
        return Err(StatusCode::FORBIDDEN.into());
    }
    match chat_detail(&state, id).await {
        Some(detail) => Ok(Json(detail)),
        None => Err(StatusCode::NOT_FOUND.into()),
    }
}

/// Деталь чата: сам чат, сообщения, участники и начатые в нём нити.
/// Нити разворачиваются рекурсивно (своими сообщениями, участниками и
/// вложенными нитями) — страница родителя рисует их свёрнутыми у сообщения.
async fn chat_detail(state: &AppState, chat_id: i64) -> Option<serde_json::Value> {
    let chat = state.chat_store.get_chat(chat_id).await.ok()?;
    let messages = state.chat_store.list_messages(chat_id).await.ok()?;
    let participants = state
        .chat_store
        .list_participants(chat_id)
        .await
        .unwrap_or_default();
    let mut threads = Vec::new();
    for thread in state.chat_store.list_threads(chat_id).await.ok()? {
        if let Some(detail) = chat_detail_boxed(state, thread.id).await {
            threads.push(detail);
        }
    }
    Some(serde_json::json!({
        "chat": chat,
        "messages": messages,
        "participants": participants,
        "threads": threads,
    }))
}

/// Рекурсия `chat_detail` через boxed-фьючер: бесконечный размер типа
/// (рекурсивный async fn) так не собирается.
fn chat_detail_boxed(
    state: &AppState,
    chat_id: i64,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<serde_json::Value>> + Send + '_>> {
    Box::pin(chat_detail(state, chat_id))
}

pub(super) async fn close_chat(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let user_id = current_user(&state, &headers).await?;
    match state
        .chat_store
        .close_workstation_session(id, user_id)
        .await
    {
        Ok(()) => {
            publish_lifecycle(&state, "session_closed", id).await;
            Ok(StatusCode::OK)
        }
        Err(SessionError::NotFound) => Err(StatusCode::NOT_FOUND.into()),
        Err(SessionError::Forbidden) => Err(StatusCode::FORBIDDEN.into()),
        Err(e) => Err(AppError::internal(e)),
    }
}

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub body: String,
    #[serde(default)]
    pub parent_id: Option<i64>,
}

#[derive(Serialize)]
pub struct SendMessageResponse {
    pub message: Message,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub invited: Vec<String>,
}

pub(super) async fn send_message(
    Path(chat_id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, AppError> {
    let user_id = current_user(&state, &headers).await?;
    if !can_write(&state, chat_id, user_id).await {
        return Err(StatusCode::FORBIDDEN.into());
    }

    let mut invited: Vec<String> = Vec::new();

    // Команды — обычные сообщения с дополнительной реакцией.
    if let Some(cmd) = parse_command(&payload.body) {
        match cmd {
            ChatCommand::Invite(name) => {
                if let Some(user) = state.chat_store.find_user_by_name(&name).await? {
                    state.chat_store.add_participant(chat_id, user.id).await?;
                    invited.push(name);
                }
            }
            ChatCommand::Kick(name) => {
                let is_super = state
                    .chat_store
                    .is_super_user(user_id)
                    .await
                    .unwrap_or(false);
                let is_owner = state
                    .chat_store
                    .is_owner(chat_id, user_id)
                    .await
                    .unwrap_or(false);
                if is_super || is_owner {
                    if let Some(user) = state.chat_store.find_user_by_name(&name).await? {
                        state
                            .chat_store
                            .remove_participant(chat_id, user.id)
                            .await?;
                    }
                }
            }
            ChatCommand::End => {
                let is_super = state
                    .chat_store
                    .is_super_user(user_id)
                    .await
                    .unwrap_or(false);
                let is_owner = state
                    .chat_store
                    .is_owner(chat_id, user_id)
                    .await
                    .unwrap_or(false);
                if is_super || is_owner {
                    state.chat_store.close_chat(chat_id).await?;
                }
            }
        }
    }

    let hidden = hidden_from_shortcuts(&state, &payload.body).await;
    if let Some(message) = state
        .chat_store
        .send_message(
            chat_id,
            user_id,
            &payload.body,
            &hidden,
            payload.parent_id,
            None,
        )
        .await?
    {
        // Чат не знает про агентов: только публикует событие в каналы. Подписчик
        // (`aga agent`) сам решает, запускать ли агента по привязке к автору.
        state
            .centrifuge
            .publish_message(chat_id, message.id, user_id)
            .await;

        Ok(Json(SendMessageResponse { message, invited }))
    } else {
        Err(StatusCode::BAD_REQUEST.into())
    }
}

#[derive(Deserialize)]
pub struct StartThreadRequest {
    pub title: String,
    pub body: String,
}

#[derive(Serialize)]
pub struct StartThreadResponse {
    pub chat: Chat,
    pub message: Message,
}

/// Начать нить от сообщения: дочерний чат с привязкой к сообщению и первым
/// сообщением, несущим заголовок нити.
pub(super) async fn start_message_thread(
    Path(message_id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<StartThreadRequest>,
) -> Result<Json<StartThreadResponse>, AppError> {
    let user_id = current_user(&state, &headers).await?;
    let Some(origin) = state.chat_store.get_message(message_id).await? else {
        return Err(StatusCode::NOT_FOUND.into());
    };
    if !can_write(&state, origin.chat_id, user_id).await {
        return Err(StatusCode::FORBIDDEN.into());
    }
    let hidden = hidden_from_shortcuts(&state, &payload.body).await;
    let Some((chat, message)) = state
        .chat_store
        .start_thread(
            origin.chat_id,
            message_id,
            &payload.title,
            &payload.body,
            &hidden,
            user_id,
        )
        .await?
    else {
        return Err(StatusCode::BAD_REQUEST.into());
    };

    // Первое сообщение нити — обычное сообщение: событие уходит в каналы,
    // агент-процесс реагирует по привязке к автору, как в любом чате.
    // Обновление и родителю (появилась свёрнутая нить), и самой нити.
    state
        .centrifuge
        .publish_message(origin.chat_id, message.id, user_id)
        .await;
    state
        .centrifuge
        .publish_message(chat.id, message.id, user_id)
        .await;
    Ok(Json(StartThreadResponse { chat, message }))
}

/// Отправить сообщение нити в родительский чат: копия остаётся в нити,
/// в ленте родителя появляется её копия со ссылкой на сообщение-источник.
pub(super) async fn post_message_to_parent(
    Path(message_id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Message>, AppError> {
    let user_id = current_user(&state, &headers).await?;
    let Some(original) = state.chat_store.get_message(message_id).await? else {
        return Err(StatusCode::NOT_FOUND.into());
    };
    let Some(thread) = state.chat_store.get_chat(original.chat_id).await? else {
        return Err(StatusCode::NOT_FOUND.into());
    };
    let Some(parent_id) = thread.parent_id else {
        return Err(StatusCode::BAD_REQUEST.into());
    };
    if !can_write(&state, parent_id, user_id).await {
        return Err(StatusCode::FORBIDDEN.into());
    }
    let Some(message) = state.chat_store.post_to_parent(message_id).await? else {
        return Err(StatusCode::BAD_REQUEST.into());
    };
    state
        .centrifuge
        .publish_message(parent_id, message.id, message.author_id)
        .await;
    Ok(Json(message))
}

pub(super) async fn list_chat_messages(
    Path(chat_id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Message>>, AppError> {
    let user_id = current_user(&state, &headers).await?;
    if !can_read(&state, chat_id, user_id).await {
        return Err(StatusCode::FORBIDDEN.into());
    }
    Ok(Json(state.chat_store.list_messages(chat_id).await?))
}

#[derive(Deserialize)]
pub struct AddParticipantRequest {
    pub user_id: i64,
}

pub(super) async fn add_participant(
    Path(chat_id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AddParticipantRequest>,
) -> Result<StatusCode, AppError> {
    let user_id = current_user(&state, &headers).await?;
    if !can_write(&state, chat_id, user_id).await {
        return Err(StatusCode::FORBIDDEN.into());
    }
    state
        .chat_store
        .add_participant(chat_id, payload.user_id)
        .await?;
    Ok(StatusCode::OK)
}

pub(super) async fn remove_participant(
    Path((chat_id, uid)): Path<(i64, i64)>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let user_id = current_user(&state, &headers).await?;
    let is_super = state
        .chat_store
        .is_super_user(user_id)
        .await
        .unwrap_or(false);
    let is_owner = state
        .chat_store
        .is_owner(chat_id, user_id)
        .await
        .unwrap_or(false);
    if !(is_super || is_owner) {
        return Err(StatusCode::FORBIDDEN.into());
    }
    if state.chat_store.remove_participant(chat_id, uid).await? {
        Ok(StatusCode::OK)
    } else {
        Err(StatusCode::NOT_FOUND.into())
    }
}

#[derive(Deserialize)]
pub struct ShareRequest {
    pub target_chat_id: i64,
}

pub(super) async fn share_message(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ShareRequest>,
) -> Result<Json<Message>, AppError> {
    let user_id = current_user(&state, &headers).await?;
    // Нужна видимость исходного сообщения.
    let Some(original) = state.chat_store.get_message(id).await? else {
        return Err(StatusCode::NOT_FOUND.into());
    };
    if !can_read(&state, original.chat_id, user_id).await {
        return Err(StatusCode::FORBIDDEN.into());
    }
    // Нужен доступ к целевому чату.
    if !can_write(&state, payload.target_chat_id, user_id).await {
        return Err(StatusCode::FORBIDDEN.into());
    }
    match state
        .chat_store
        .share_message(payload.target_chat_id, id, user_id)
        .await?
    {
        Some(msg) => {
            state
                .centrifuge
                .publish_message(msg.chat_id, msg.id, msg.author_id)
                .await;
            Ok(Json(msg))
        }
        None => Err(StatusCode::BAD_REQUEST.into()),
    }
}

pub(super) async fn message_artifacts(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::chat::Artifact>>, AppError> {
    current_user(&state, &headers).await?;
    Ok(Json(state.chat_store.list_artifacts(id).await?))
}

/// Скрытая часть сообщения из сокращений: каждое слово `/имя` в тексте
/// добавляет текст привязанного сокращения (одно имя — один раз). Неизвестные
/// имена и пустой текст пропускаются. Скрытая часть — заметка: в контекст
/// агентов (`build_context`) она не входит.
async fn hidden_from_shortcuts(state: &AppState, body: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for name in crate::chat::shortcut_names(body) {
        if let Ok(Some(content)) = state
            .trace_store
            .resolve_capability(crate::trace::CapabilityKind::Shortcut, &name)
            .await
        {
            if !content.is_empty() {
                parts.push(content);
            }
        }
    }
    parts.join("\n")
}
