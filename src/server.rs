use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio::sync::mpsc;
use tower_http::services::ServeDir;

use crate::agent::Agent;
use crate::auth;
use crate::chat::{parse_command, Chat, ChatCommand, ChatStore, Message};
use crate::config::Config;
use crate::llm::LlmClient;
use crate::reactive::ReactiveRunner;
use crate::trace::TraceStore;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub trace_store: TraceStore,
    pub llm_client: LlmClient,
    pub chat_store: ChatStore,
    pub reactive: ReactiveRunner,
}

fn sso_enabled(state: &AppState) -> bool {
    state
        .config
        .sso
        .as_ref()
        .map(|s| s.enabled)
        .unwrap_or(false)
}

async fn current_user(state: &AppState, headers: &HeaderMap) -> i64 {
    auth::resolve_user(headers, &state.chat_store, sso_enabled(state)).await
}

#[derive(Deserialize)]
pub struct TaskRequest {
    pub task: String,
    #[allow(dead_code)]
    pub project_id: Option<i64>,
}

#[derive(Serialize)]
pub struct TaskResponse {
    pub status: String,
    pub task_id: String,
    pub result: String,
}

#[derive(Deserialize)]
pub struct HumanAnswerRequest {
    pub answer: String,
}

#[derive(Serialize)]
pub struct ProjectInfo {
    pub id: i64,
    pub compose_path: String,
    pub active_roles: Vec<String>,
}

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    pub compose_path: String,
}

#[derive(Deserialize)]
pub struct SetProjectRolesRequest {
    pub active_roles: Vec<String>,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/tasks/:role", post(create_task))
        .route("/trace/:task_id", get(get_trace))
        .route("/human/pending", get(pending_human_requests))
        .route("/human/answer/:id", post(answer_human_request))
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/:id", get(get_project).delete(delete_project))
        .route(
            "/projects/:id/roles",
            get(get_project_roles).post(set_project_roles),
        )
        .route("/roles", get(list_all_roles))
        // === Модель чата ===
        .route("/users", get(list_users))
        .route("/users/:id", get(get_user))
        .route("/chats", get(list_chats).post(create_chat))
        .route("/chats/:id", get(get_chat))
        .route("/chats/:id/close", post(close_chat))
        .route(
            "/chats/:id/messages",
            get(list_chat_messages).post(send_message),
        )
        .route("/chats/:id/participants", post(add_participant))
        .route(
            "/chats/:id/participants/:uid",
            axum::routing::delete(remove_participant),
        )
        .route("/messages/:id/share", post(share_message))
        .route("/messages/:id/artifacts", get(message_artifacts))
        .route(
            "/workstations",
            get(list_workstations).post(create_workstation),
        )
        .nest_service("/", ServeDir::new("static"))
        .with_state(state)
}

async fn create_task(
    Path(role): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<TaskRequest>,
) -> Result<Json<TaskResponse>, StatusCode> {
    let role_config = state
        .config
        .get_role(&role)
        .ok_or(StatusCode::NOT_FOUND)?
        .clone();

    let task_id = uuid::Uuid::new_v4().to_string();

    let agent = Agent::new(
        role_config,
        state.llm_client.clone(),
        state.trace_store.clone(),
    );

    match agent.run(&task_id, &payload.task).await {
        Ok(result) => Ok(Json(TaskResponse {
            status: "ok".to_string(),
            task_id,
            result,
        })),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_trace(
    Path(task_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.trace_store.get_trace(&task_id).await {
        Ok(Some(trace)) => Ok(Json(serde_json::to_value(trace).unwrap())),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn pending_human_requests(
    State(state): State<AppState>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let (tx, rx) = mpsc::channel::<String>(100);

    if let Ok(requests) = state.trace_store.get_pending_human_requests().await {
        for (id, task_id, question) in requests {
            let json = serde_json::json!({
                "id": id,
                "task_id": task_id,
                "question": question,
            });
            let _ = tx.send(format!("data: {}\n\n", json)).await;
        }
    }

    let stream = stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|data| (Ok(Event::default().data(data)), rx))
    });

    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}

async fn answer_human_request(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<HumanAnswerRequest>,
) -> Result<StatusCode, StatusCode> {
    match state
        .trace_store
        .answer_human_request(&id, &payload.answer)
        .await
    {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

// === API для управления проектами ===

async fn list_projects(
    State(state): State<AppState>,
) -> Result<Json<Vec<ProjectInfo>>, StatusCode> {
    match state.trace_store.get_all_projects().await {
        Ok(projects) => {
            let mut result = Vec::new();
            for project in projects {
                let active_roles = state
                    .trace_store
                    .get_active_project_roles(project.id)
                    .await
                    .unwrap_or_default();
                result.push(ProjectInfo {
                    id: project.id,
                    compose_path: project.compose_path,
                    active_roles,
                });
            }
            Ok(Json(result))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn create_project(
    State(state): State<AppState>,
    Json(payload): Json<CreateProjectRequest>,
) -> Result<Json<ProjectInfo>, StatusCode> {
    match state
        .trace_store
        .upsert_project(&payload.compose_path)
        .await
    {
        Ok(project_id) => {
            let active_roles = state
                .trace_store
                .get_active_project_roles(project_id)
                .await
                .unwrap_or_default();
            Ok(Json(ProjectInfo {
                id: project_id,
                compose_path: payload.compose_path,
                active_roles,
            }))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_project(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> Result<Json<ProjectInfo>, StatusCode> {
    match state.trace_store.get_project(id).await {
        Ok(Some(project)) => {
            let active_roles = state
                .trace_store
                .get_active_project_roles(project.id)
                .await
                .unwrap_or_default();
            Ok(Json(ProjectInfo {
                id: project.id,
                compose_path: project.compose_path,
                active_roles,
            }))
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn delete_project(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    match state.trace_store.delete_project(id).await {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_project_roles(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> Result<Json<Vec<String>>, StatusCode> {
    match state.trace_store.get_active_project_roles(id).await {
        Ok(roles) => Ok(Json(roles)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn set_project_roles(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Json(payload): Json<SetProjectRolesRequest>,
) -> Result<StatusCode, StatusCode> {
    let roles_refs: Vec<&str> = payload.active_roles.iter().map(|s| s.as_str()).collect();
    match state.trace_store.set_project_roles(id, &roles_refs).await {
        Ok(_) => Ok(StatusCode::OK),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn list_all_roles(State(state): State<AppState>) -> Result<Json<Vec<String>>, StatusCode> {
    let roles: Vec<String> = state.config.roles.keys().cloned().collect();
    Ok(Json(roles))
}

// === Модель чата ===

async fn list_users(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::chat::ChatUser>>, StatusCode> {
    state
        .chat_store
        .list_users()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_user(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> Result<Json<crate::chat::ChatUser>, StatusCode> {
    match state.chat_store.get_user(id).await {
        Ok(Some(u)) => Ok(Json(u)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(Deserialize)]
pub struct CreateChatRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub workstation_id: Option<i64>,
}

async fn create_chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateChatRequest>,
) -> Result<Json<Chat>, StatusCode> {
    let user_id = current_user(&state, &headers).await;
    match state
        .chat_store
        .create_chat(
            payload.parent_id,
            payload.title.as_deref(),
            user_id,
            payload.workstation_id,
        )
        .await
    {
        Ok(chat) => Ok(Json(chat)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn list_chats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Chat>>, StatusCode> {
    let user_id = current_user(&state, &headers).await;
    state
        .chat_store
        .list_chats_for_user(user_id)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_chat(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let user_id = current_user(&state, &headers).await;
    if !can_read(&state, id, user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }
    let Some(chat) = state
        .chat_store
        .get_chat(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Err(StatusCode::NOT_FOUND);
    };
    let messages = state
        .chat_store
        .list_messages(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let participants = state
        .chat_store
        .list_participants(id)
        .await
        .unwrap_or_default();
    Ok(Json(serde_json::json!({
        "chat": chat,
        "messages": messages,
        "participants": participants,
    })))
}

async fn close_chat(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    let user_id = current_user(&state, &headers).await;
    if !can_write(&state, id, user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }
    let is_super = state
        .chat_store
        .is_super_user(user_id)
        .await
        .unwrap_or(false);
    let is_owner = state
        .chat_store
        .is_owner(id, user_id)
        .await
        .unwrap_or(false);
    if !is_super && !is_owner {
        return Err(StatusCode::FORBIDDEN);
    }
    match state.chat_store.close_chat(id).await {
        Ok(true) => Ok(StatusCode::OK),
        _ => Err(StatusCode::INTERNAL_SERVER_ERROR),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_chat: Option<Chat>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub invited: Vec<String>,
}

async fn send_message(
    Path(chat_id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, StatusCode> {
    let user_id = current_user(&state, &headers).await;
    if !can_write(&state, chat_id, user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut created_chat: Option<Chat> = None;
    let mut invited: Vec<String> = Vec::new();

    // Команды — обычные сообщения с дополнительной реакцией.
    if let Some(cmd) = parse_command(&payload.body) {
        match cmd {
            ChatCommand::Invite(name) => {
                if let Some(user) = state
                    .chat_store
                    .find_user_by_name(&name)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                {
                    state
                        .chat_store
                        .add_participant(chat_id, user.id)
                        .await
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
                    if let Some(user) = state
                        .chat_store
                        .find_user_by_name(&name)
                        .await
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                    {
                        state
                            .chat_store
                            .remove_participant(chat_id, user.id)
                            .await
                            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                    }
                }
            }
            ChatCommand::Start(title) => {
                if let Ok(chat) = state
                    .chat_store
                    .create_chat(Some(chat_id), Some(&title), user_id, None)
                    .await
                {
                    created_chat = Some(chat);
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
                    state
                        .chat_store
                        .close_chat(chat_id)
                        .await
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                }
            }
        }
    }

    if let Some(message) = state
        .chat_store
        .send_message(chat_id, user_id, &payload.body, payload.parent_id, None)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        // Реактивные агенты по упоминаниям @Agent.<role>.
        for role in crate::chat::mentioned_roles(&payload.body) {
            if let Ok(agent_user_id) = state.chat_store.ensure_agent_user(&role).await {
                let context = build_context(&state, chat_id)
                    .await
                    .unwrap_or_else(|| payload.body.clone());
                state
                    .reactive
                    .enqueue(chat_id, &role, agent_user_id, context);
            }
        }

        Ok(Json(SendMessageResponse {
            message,
            created_chat,
            invited,
        }))
    } else {
        Err(StatusCode::BAD_REQUEST)
    }
}

async fn list_chat_messages(
    Path(chat_id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Message>>, StatusCode> {
    let user_id = current_user(&state, &headers).await;
    if !can_read(&state, chat_id, user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .chat_store
        .list_messages(chat_id)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Deserialize)]
pub struct AddParticipantRequest {
    pub user_id: i64,
}

async fn add_participant(
    Path(chat_id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AddParticipantRequest>,
) -> Result<StatusCode, StatusCode> {
    let user_id = current_user(&state, &headers).await;
    if !can_write(&state, chat_id, user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .chat_store
        .add_participant(chat_id, payload.user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

async fn remove_participant(
    Path((chat_id, uid)): Path<(i64, i64)>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    let user_id = current_user(&state, &headers).await;
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
        return Err(StatusCode::FORBIDDEN);
    }
    match state.chat_store.remove_participant(chat_id, uid).await {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(Deserialize)]
pub struct ShareRequest {
    pub target_chat_id: i64,
}

async fn share_message(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ShareRequest>,
) -> Result<Json<Message>, StatusCode> {
    let user_id = current_user(&state, &headers).await;
    // Нужна видимость исходного сообщения.
    let Some(original) = state
        .chat_store
        .get_message(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Err(StatusCode::NOT_FOUND);
    };
    if !can_read(&state, original.chat_id, user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }
    // Нужен доступ к целевому чату.
    if !can_write(&state, payload.target_chat_id, user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }
    match state
        .chat_store
        .share_message(payload.target_chat_id, id, user_id)
        .await
    {
        Ok(Some(msg)) => Ok(Json(msg)),
        Ok(None) => Err(StatusCode::BAD_REQUEST),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn message_artifacts(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::chat::Artifact>>, StatusCode> {
    state
        .chat_store
        .list_artifacts(id)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn list_workstations(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::chat::Workstation>>, StatusCode> {
    state
        .chat_store
        .list_workstations()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Deserialize)]
pub struct CreateWorkstationRequest {
    pub project_id: i64,
    #[serde(default)]
    pub name: Option<String>,
}

async fn create_workstation(
    State(state): State<AppState>,
    Json(payload): Json<CreateWorkstationRequest>,
) -> Result<Json<crate::chat::Workstation>, StatusCode> {
    let name = payload.name.unwrap_or_else(|| "ws".to_string());
    match state
        .chat_store
        .create_workstation(payload.project_id, &name)
        .await
    {
        Ok(ws) => Ok(Json(ws)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

// === Permission helpers ===

/// Читать чат можно участникам и суперпользователю.
async fn can_read(state: &AppState, chat_id: i64, user_id: i64) -> bool {
    if state
        .chat_store
        .is_super_user(user_id)
        .await
        .unwrap_or(false)
    {
        return true;
    }
    state
        .chat_store
        .is_participant(chat_id, user_id)
        .await
        .unwrap_or(false)
}

/// Писать в открытый чат можно участникам и суперпользователю.
async fn can_write(state: &AppState, chat_id: i64, user_id: i64) -> bool {
    if state
        .chat_store
        .is_super_user(user_id)
        .await
        .unwrap_or(false)
    {
        return true;
    }
    let is_open = state
        .chat_store
        .get_chat(chat_id)
        .await
        .ok()
        .flatten()
        .map(|c| c.state == "OPEN")
        .unwrap_or(false);
    is_open
        && state
            .chat_store
            .is_participant(chat_id, user_id)
            .await
            .unwrap_or(false)
}

async fn build_context(state: &AppState, chat_id: i64) -> Option<String> {
    let messages = state.chat_store.list_messages(chat_id).await.ok()?;
    let tail: Vec<String> = messages
        .iter()
        .rev()
        .take(10)
        .rev()
        .map(|m| m.body.clone())
        .collect();
    Some(tail.join("\n"))
}
