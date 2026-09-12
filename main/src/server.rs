use axum::body::Body;
use axum::{
    extract::{Path, State},
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderMap, HeaderValue, Method, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::auth;
use crate::centrifuge::CentrifugeClient;
use crate::chat::{parse_command, Chat, ChatCommand, ChatStore, Message, SessionError};
use crate::cluster::Cluster;
use crate::config::Config;
use crate::trace::TraceStore;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub trace_store: TraceStore,
    pub chat_store: ChatStore,
    pub cluster: Cluster,
    /// Клиент Centrifugo (реальное время для чата). Не настроен — `disabled()`.
    pub centrifuge: CentrifugeClient,
    /// Верификатор JWT против JWKS; обновляется фоновой задачей (Keycloak
    /// пересоздаёт ключ подписи при переимпорте realm). None — SSO выключен.
    pub sso_verifier: Arc<RwLock<Option<auth::JwtVerifier>>>,
    /// Origin веб-клиента (front/): CORS-источник и адрес возврата токена после SSO.
    pub front_url: String,
}

/// Текущий пользователь: без SSO — аноним-суперпользователь; с SSO —
/// участник из токена. Недействительный токен — 401.
async fn current_user(state: &AppState, headers: &HeaderMap) -> Result<i64, StatusCode> {
    let verifier = state.sso_verifier.read().await;
    auth::resolve_user(headers, &state.chat_store, verifier.as_ref()).await
}

/// Текущий пользователь с именем для истории изменений каталога: имя
/// фиксируется в момент правки, чтобы историю не исказили переименования.
async fn current_actor(state: &AppState, headers: &HeaderMap) -> Result<(i64, String), StatusCode> {
    let id = current_user(state, headers).await?;
    let name = state
        .chat_store
        .get_user(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(|u| u.name)
        .unwrap_or_else(|| format!("#{id}"));
    Ok((id, name))
}

#[derive(Serialize)]
pub struct ProjectInfo {
    pub id: i64,
    pub git_url: String,
    /// Прикреплённый набор агентов (или null).
    pub agent_set: Option<crate::trace::AgentSet>,
}

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    pub git_url: String,
}

#[derive(Deserialize)]
pub struct AttachAgentSetRequest {
    pub agent_set_id: i64,
}

#[derive(Deserialize)]
pub struct CreateAgentSetRequest {
    pub name: String,
    pub agents: Vec<crate::trace::AgentSpec>,
}

#[derive(Deserialize)]
pub struct LlmConnectionRequest {
    pub name: String,
    pub api_url: String,
    pub api_key: Option<String>,
    pub model_name: String,
}

/// Выбор дефолтной LLM на странице «LLM»: id подключения или null (снять выбор).
#[derive(Deserialize)]
pub struct SetLlmDefaultRequest {
    pub llm_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct CreateCapabilityRequest {
    pub name: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct UpdateCapabilityRequest {
    pub name: Option<String>,
    pub content: Option<String>,
}

/// Фильтр «Удалённые»: `?deleted=1` — только удалённые записи каталога.
#[derive(Deserialize)]
pub struct DeletedFilter {
    pub deleted: Option<i64>,
}

pub fn create_router(state: AppState) -> Router {
    // SPA (front/) живёт на другом origin — CORS пропускает его. Разрешаем
    // и dev.localhost (k8s-стенд и dev-прокси), и front_url из env (например
    // localhost:8081 в dev-стенде) — браузер может открывать SPA обоими путями.
    let mut origins = vec![state
        .front_url
        .parse::<HeaderValue>()
        .unwrap_or_else(|_| HeaderValue::from_static("http://dev.localhost"))];
    let dev_localhost = HeaderValue::from_static("http://dev.localhost");
    if !origins.contains(&dev_localhost) {
        origins.push(dev_localhost);
    }
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers([CONTENT_TYPE, AUTHORIZATION]);
    Router::new()
        .route("/trace/:task_id", get(get_trace))
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/:id", get(get_project).delete(delete_project))
        .route(
            "/projects/:id/agent-set",
            get(get_project_agent_set).post(attach_agent_set),
        )
        .route("/agent-sets", get(list_agent_sets).post(create_agent_set))
        .route(
            "/agent-sets/:id",
            get(get_agent_set)
                .delete(delete_agent_set)
                .patch(update_agent_set),
        )
        // === Подключения к LLM ===
        // Имя, url API, ключ и модель. Агент набора ссылается на подключение;
        // без него агент ходит к дефолтной LLM (одно из подключений отмечается
        // на странице «LLM»; env-дефолта больше нет).
        .route(
            "/llms",
            get(list_llm_connections).post(create_llm_connection),
        )
        .route(
            "/llms/:id",
            get(get_llm_connection)
                .delete(delete_llm_connection)
                .patch(update_llm_connection),
        )
        .route("/settings/llm-default", post(set_llm_default))
        // === Каталог способностей (скиллы и команды) ===
        // У записи одно текущее содержимое и история изменений (кто, когда и
        // что сделал). ?deleted=1 — список «Удалённые» (история переживает
        // удаление). Фиксации версий нет: агент всегда берёт последнее.
        .route("/skills", get(list_skills).post(create_skill))
        .route(
            "/skills/:id",
            get(get_skill).patch(update_skill).delete(delete_skill),
        )
        .route("/skills/:id/history", get(capability_history))
        .route("/commands", get(list_commands).post(create_command))
        .route(
            "/commands/:id",
            get(get_command)
                .patch(update_command)
                .delete(delete_command),
        )
        .route("/commands/:id/history", get(capability_history))
        // === Сокращения (shortcuts) ===
        // Глобальный перечень: при отправке сообщения слово `/имя` добавляет
        // привязанный текст в скрытую часть сообщения. Живёт в том же каталоге,
        // что скиллы и команды (kind='shortcut'), с той же историей изменений;
        // агентам не даётся. Имя — без пробелов, уникально.
        .route("/shortcuts", get(list_shortcuts).post(create_shortcut))
        .route(
            "/shortcuts/:id",
            get(get_shortcut)
                .patch(update_shortcut)
                .delete(delete_shortcut),
        )
        .route("/shortcuts/:id/history", get(capability_history))
        // === SSO (Keycloak): вход веб-клиента ===
        .route("/auth/login", get(auth_login))
        .route("/auth/callback", get(auth_callback))
        .route("/auth/refresh", post(auth_refresh))
        .route("/auth/logout", get(auth_logout))
        // === Модель чата ===
        .route("/users", get(list_users))
        .route("/users/me", get(me))
        .route("/users/:id", get(get_user))
        .route("/connection-jwt/", get(connection_jwt))
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
        .route("/messages/:id/thread", post(start_message_thread))
        .route("/messages/:id/to-parent", post(post_message_to_parent))
        .route("/messages/:id/artifacts", get(message_artifacts))
        .route(
            "/workstations",
            get(list_workstations).post(create_workstation),
        )
        .route(
            "/workstations/:id/session",
            get(get_workstation_session).post(open_workstation_session),
        )
        .route(
            "/workstations/:id",
            axum::routing::delete(delete_workstation),
        )
        .route("/workstations/:id/release", post(release_workstation))
        .route("/workstations/:id/down", post(mark_workstation_down))
        // === Просмотр содержимого проекта в воркстейшне ===
        .route("/workstations/:id/tree", get(workstation_tree))
        .route("/workstations/:id/file", get(workstation_file))
        .route("/workstations/:id/changes", get(workstation_changes))
        // === Настройки: публичный SSH-ключ aga ===
        .route("/settings/ssh-key", get(get_ssh_key))
        .layer(cors)
        .with_state(state)
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

// === API для управления проектами ===

async fn list_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ProjectInfo>>, StatusCode> {
    // Любой вошедший участник видит все проекты.
    current_user(&state, &headers).await?;
    match state.trace_store.get_all_projects().await {
        Ok(projects) => {
            let mut result = Vec::new();
            for project in projects {
                let agent_set = state
                    .trace_store
                    .get_project_agent_set(project.id)
                    .await
                    .unwrap_or(None);
                result.push(ProjectInfo {
                    id: project.id,
                    git_url: project.git_url,
                    agent_set,
                });
            }
            Ok(Json(result))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn create_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateProjectRequest>,
) -> Result<Json<ProjectInfo>, StatusCode> {
    current_user(&state, &headers).await?;
    match state.trace_store.upsert_project(&payload.git_url).await {
        Ok(project_id) => {
            let agent_set = state
                .trace_store
                .get_project_agent_set(project_id)
                .await
                .unwrap_or(None);
            Ok(Json(ProjectInfo {
                id: project_id,
                git_url: payload.git_url,
                agent_set,
            }))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_project(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ProjectInfo>, StatusCode> {
    current_user(&state, &headers).await?;
    match state.trace_store.get_project(id).await {
        Ok(Some(project)) => {
            let agent_set = state
                .trace_store
                .get_project_agent_set(project.id)
                .await
                .unwrap_or(None);
            Ok(Json(ProjectInfo {
                id: project.id,
                git_url: project.git_url,
                agent_set,
            }))
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn delete_project(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    current_user(&state, &headers).await?;
    match state.trace_store.delete_project(id).await {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

// === API для управления наборами агентов ===

async fn list_agent_sets(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::trace::AgentSet>>, StatusCode> {
    current_user(&state, &headers).await?;
    state
        .trace_store
        .list_agent_sets()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn create_agent_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateAgentSetRequest>,
) -> Result<Json<crate::trace::AgentSet>, StatusCode> {
    current_user(&state, &headers).await?;
    match state
        .trace_store
        .create_agent_set(&payload.name, &payload.agents)
        .await
    {
        Ok(set_id) => state
            .trace_store
            .get_agent_set(set_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
            .map(Json),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_agent_set(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::trace::AgentSet>, StatusCode> {
    current_user(&state, &headers).await?;
    state
        .trace_store
        .get_agent_set(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)
        .map(Json)
}

async fn delete_agent_set(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    current_user(&state, &headers).await?;
    match state.trace_store.delete_agent_set(id).await {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Полностью заменить состав набора (имя, агенты с их территорией,
/// инструментами и данными скиллами/командами). Возвращает обновлённый набор.
async fn update_agent_set(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateAgentSetRequest>,
) -> Result<Json<crate::trace::AgentSet>, StatusCode> {
    current_user(&state, &headers).await?;
    match state
        .trace_store
        .update_agent_set(id, &payload.name, &payload.agents)
        .await
    {
        Ok(true) => state
            .trace_store
            .get_agent_set(id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::NOT_FOUND)
            .map(Json),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

// === API для управления подключениями к LLM ===

async fn list_llm_connections(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::trace::LlmConnection>>, StatusCode> {
    current_user(&state, &headers).await?;
    state
        .trace_store
        .list_llm_connections()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn create_llm_connection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LlmConnectionRequest>,
) -> Result<Json<crate::trace::LlmConnection>, StatusCode> {
    current_user(&state, &headers).await?;
    let spec = crate::trace::LlmConnectionSpec {
        name: payload.name,
        api_url: payload.api_url,
        api_key: payload.api_key,
        model_name: payload.model_name,
    };
    match state.trace_store.create_llm_connection(&spec).await {
        Ok(id) => state
            .trace_store
            .get_llm_connection(id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
            .map(Json),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_llm_connection(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::trace::LlmConnection>, StatusCode> {
    current_user(&state, &headers).await?;
    state
        .trace_store
        .get_llm_connection(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)
        .map(Json)
}

async fn delete_llm_connection(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    current_user(&state, &headers).await?;
    match state.trace_store.delete_llm_connection(id).await {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Изменить подключение (название, url API, ключ, модель). Возвращает
/// обновлённое подключение; используется агентами набора сразу после правки.
async fn update_llm_connection(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LlmConnectionRequest>,
) -> Result<Json<crate::trace::LlmConnection>, StatusCode> {
    current_user(&state, &headers).await?;
    let spec = crate::trace::LlmConnectionSpec {
        name: payload.name,
        api_url: payload.api_url,
        api_key: payload.api_key,
        model_name: payload.model_name,
    };
    match state.trace_store.update_llm_connection(id, &spec).await {
        Ok(true) => state
            .trace_store
            .get_llm_connection(id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::NOT_FOUND)
            .map(Json),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Выбрать дефолтную LLM: `llm_id` — одно из подключений (дефолт снимается с
/// остальных — одна дефолтная LLM), `null` — снять выбор. Возвращает дефолтное
/// подключение или null.
async fn set_llm_default(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SetLlmDefaultRequest>,
) -> Result<Json<Option<crate::trace::LlmConnection>>, StatusCode> {
    current_user(&state, &headers).await?;
    match payload.llm_id {
        Some(id) => match state.trace_store.set_default_llm(id).await {
            Ok(true) => state
                .trace_store
                .get_llm_connection(id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .map(|c| Json(Some(c)))
                .ok_or(StatusCode::NOT_FOUND),
            Ok(false) => Err(StatusCode::NOT_FOUND),
            Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
        },
        None => {
            state
                .trace_store
                .clear_default_llm()
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(Json(None))
        }
    }
}

/// Общий список записей каталога вида: активные или «Удалённые» (`?deleted=1`).
async fn list_capabilities(
    kind: crate::trace::CapabilityKind,
    state: &AppState,
    headers: &HeaderMap,
    deleted: Option<i64>,
) -> Result<Json<Vec<crate::trace::CapabilityItem>>, StatusCode> {
    current_user(state, headers).await?;
    let items = match deleted {
        Some(1) => state
            .trace_store
            .list_deleted_capabilities(kind)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        _ => state
            .trace_store
            .list_capabilities(kind, false)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    };
    Ok(Json(items))
}

async fn list_skills(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(filter): axum::extract::Query<DeletedFilter>,
) -> Result<Json<Vec<crate::trace::CapabilityItem>>, StatusCode> {
    list_capabilities(
        crate::trace::CapabilityKind::Skill,
        &state,
        &headers,
        filter.deleted,
    )
    .await
}

async fn create_skill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateCapabilityRequest>,
) -> Result<Json<crate::trace::CapabilityItem>, StatusCode> {
    create_capability(
        crate::trace::CapabilityKind::Skill,
        &state,
        &headers,
        payload,
    )
    .await
}

async fn list_commands(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(filter): axum::extract::Query<DeletedFilter>,
) -> Result<Json<Vec<crate::trace::CapabilityItem>>, StatusCode> {
    list_capabilities(
        crate::trace::CapabilityKind::Command,
        &state,
        &headers,
        filter.deleted,
    )
    .await
}

async fn create_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateCapabilityRequest>,
) -> Result<Json<crate::trace::CapabilityItem>, StatusCode> {
    create_capability(
        crate::trace::CapabilityKind::Command,
        &state,
        &headers,
        payload,
    )
    .await
}

async fn create_capability(
    kind: crate::trace::CapabilityKind,
    state: &AppState,
    headers: &HeaderMap,
    payload: CreateCapabilityRequest,
) -> Result<Json<crate::trace::CapabilityItem>, StatusCode> {
    let (actor, actor_name) = current_actor(state, headers).await?;
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Имя в пределах вида уникально (в т.ч. среди удалённых) — дубль это 409.
    if state
        .trace_store
        .capability_name_taken(kind, &name, 0)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Err(StatusCode::CONFLICT);
    }
    let id = state
        .trace_store
        .create_capability(kind, &name, &payload.content, actor, &actor_name)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    state
        .trace_store
        .get_capability(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
        .map(Json)
}

async fn get_skill(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::trace::CapabilityItem>, StatusCode> {
    current_user(&state, &headers).await?;
    state
        .trace_store
        .get_capability(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter(|c| c.kind == crate::trace::CapabilityKind::Skill)
        .ok_or(StatusCode::NOT_FOUND)
        .map(Json)
}

async fn get_command(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::trace::CapabilityItem>, StatusCode> {
    current_user(&state, &headers).await?;
    state
        .trace_store
        .get_capability(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter(|c| c.kind == crate::trace::CapabilityKind::Command)
        .ok_or(StatusCode::NOT_FOUND)
        .map(Json)
}

/// Правка записи: имя и/или содержимое; каждое изменение пишет запись истории.
async fn update_skill(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdateCapabilityRequest>,
) -> Result<Json<crate::trace::CapabilityItem>, StatusCode> {
    update_capability(
        crate::trace::CapabilityKind::Skill,
        id,
        &state,
        &headers,
        payload,
    )
    .await
}

async fn update_command(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdateCapabilityRequest>,
) -> Result<Json<crate::trace::CapabilityItem>, StatusCode> {
    update_capability(
        crate::trace::CapabilityKind::Command,
        id,
        &state,
        &headers,
        payload,
    )
    .await
}

async fn update_capability(
    kind: crate::trace::CapabilityKind,
    id: i64,
    state: &AppState,
    headers: &HeaderMap,
    payload: UpdateCapabilityRequest,
) -> Result<Json<crate::trace::CapabilityItem>, StatusCode> {
    let (actor, actor_name) = current_actor(state, headers).await?;
    let current = state
        .trace_store
        .get_capability(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter(|c| c.kind == kind)
        .ok_or(StatusCode::NOT_FOUND)?;
    // Имя меняем только если оно реально изменилось: правка содержимого с тем же
    // именем не должна писать в историю «переименовал». Совпадение с уже занятым
    // именем (активным или удалённым) — 409, а не 500.
    if let Some(name) = payload
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
    {
        if name != current.name {
            if state
                .trace_store
                .capability_name_taken(kind, &name, id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            {
                return Err(StatusCode::CONFLICT);
            }
            if !state
                .trace_store
                .rename_capability(id, &name, actor, &actor_name)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            {
                return Err(StatusCode::NOT_FOUND);
            }
        }
    }
    if let Some(content) = payload.content {
        if !state
            .trace_store
            .update_capability_content(id, &content, actor, &actor_name)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        {
            return Err(StatusCode::NOT_FOUND);
        }
    }
    state
        .trace_store
        .get_capability(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)
        .map(Json)
}

async fn delete_skill(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    delete_capability(crate::trace::CapabilityKind::Skill, id, &state, &headers).await
}

async fn delete_command(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    delete_capability(crate::trace::CapabilityKind::Command, id, &state, &headers).await
}

async fn delete_capability(
    kind: crate::trace::CapabilityKind,
    id: i64,
    state: &AppState,
    headers: &HeaderMap,
) -> Result<StatusCode, StatusCode> {
    let (actor, actor_name) = current_actor(state, headers).await?;
    let exists = state
        .trace_store
        .get_capability(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter(|c| c.kind == kind)
        .is_some();
    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }
    match state
        .trace_store
        .delete_capability(id, actor, &actor_name)
        .await
    {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// История изменений записи каталога: кто, когда и что сделал, по порядку.
async fn capability_history(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::trace::CapabilityHistoryEntry>>, StatusCode> {
    current_user(&state, &headers).await?;
    if state
        .trace_store
        .get_capability(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .is_none()
    {
        return Err(StatusCode::NOT_FOUND);
    }
    state
        .trace_store
        .capability_history(id)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// === Сокращения (shortcuts) ===
// Живут в общем каталоге (kind='shortcut') с той же историей, что скиллы и
// команды. Отличие — имя не содержит пробелов: вызывается оно словом `/имя`,
// которое браузер не разобьёт на части.

/// Имя сокращения: непустое и без пробелов.
fn shortcut_name_valid(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(|c| c.is_whitespace())
}

async fn list_shortcuts(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(filter): axum::extract::Query<DeletedFilter>,
) -> Result<Json<Vec<crate::trace::CapabilityItem>>, StatusCode> {
    list_capabilities(
        crate::trace::CapabilityKind::Shortcut,
        &state,
        &headers,
        filter.deleted,
    )
    .await
}

async fn create_shortcut(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateCapabilityRequest>,
) -> Result<Json<crate::trace::CapabilityItem>, StatusCode> {
    let name = payload.name.trim().to_string();
    if !shortcut_name_valid(&name) {
        return Err(StatusCode::BAD_REQUEST);
    }
    create_capability(
        crate::trace::CapabilityKind::Shortcut,
        &state,
        &headers,
        CreateCapabilityRequest {
            name,
            content: payload.content,
        },
    )
    .await
}

async fn get_shortcut(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::trace::CapabilityItem>, StatusCode> {
    current_user(&state, &headers).await?;
    state
        .trace_store
        .get_capability(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter(|c| c.kind == crate::trace::CapabilityKind::Shortcut)
        .ok_or(StatusCode::NOT_FOUND)
        .map(Json)
}

async fn update_shortcut(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdateCapabilityRequest>,
) -> Result<Json<crate::trace::CapabilityItem>, StatusCode> {
    if let Some(name) = payload.name.as_deref() {
        if !name.trim().is_empty() && !shortcut_name_valid(name.trim()) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    update_capability(
        crate::trace::CapabilityKind::Shortcut,
        id,
        &state,
        &headers,
        payload,
    )
    .await
}

async fn delete_shortcut(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    delete_capability(crate::trace::CapabilityKind::Shortcut, id, &state, &headers).await
}

async fn attach_agent_set(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AttachAgentSetRequest>,
) -> Result<StatusCode, StatusCode> {
    current_user(&state, &headers).await?;
    if state
        .trace_store
        .get_project(id)
        .await
        .unwrap_or(None)
        .is_none()
    {
        return Err(StatusCode::NOT_FOUND);
    }
    match state
        .trace_store
        .attach_agent_set(id, payload.agent_set_id)
        .await
    {
        Ok(_) => Ok(StatusCode::OK),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_project_agent_set(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::trace::AgentSet>, StatusCode> {
    current_user(&state, &headers).await?;
    state
        .trace_store
        .get_project_agent_set(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)
        .map(Json)
}

// === SSO (Keycloak): вход веб-клиента ===

/// Абсолютный `redirect_uri` для SSO-флоу: `{scheme}://{host}/auth/callback`.
/// scheme берётся из `X-Forwarded-Proto` (когда запрос идёт через ingress/прокси),
/// иначе http; host — из заголовка `Host`. Относительный redirect_uri Keycloak
/// резолвит в свой собственный хост (auth.localhost) — после входа браузер
/// попадает на Keycloak и получает 404 "Page not found". Поэтому хост обязателен.
fn sso_redirect_uri(headers: &HeaderMap) -> Option<String> {
    let host = headers.get(axum::http::header::HOST)?.to_str().ok()?;
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    Some(format!("{scheme}://{host}/auth/callback"))
}

/// Начать вход: редирект на authorize-эндпоинт Keycloak. Токен после входа
/// возвращается веб-клиенту (front_url/#token=...) — см. auth_callback.
///
/// Параметры `prompt=none` и `silent=1` — silent-флоу обновления токена из
/// скрытого iframe: `prompt=none` заставляет Keycloak вернуть код (или ошибку)
/// без формы входа, а `silent` echo-ится в redirect_uri, чтобы колбэк ответил
/// страницей postMessage, а не редиректом на фронт.
async fn auth_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<axum::response::Redirect, StatusCode> {
    let sso = state
        .config
        .sso
        .as_ref()
        .filter(|s| s.enabled)
        .ok_or(StatusCode::NOT_FOUND)?;
    let authorize_url = sso.authorize_url.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    let client_id = sso.client_id.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    // Куда Keycloak вернёт после входа — абсолютный адрес из заголовка Host
    // (Origin при обычной навигации браузер не шлёт).
    let redirect_uri = sso_redirect_uri(&headers).ok_or(StatusCode::NOT_FOUND)?;
    let redirect_uri = if params.contains_key("silent") {
        format!("{redirect_uri}?silent=1")
    } else {
        redirect_uri
    };
    let mut url = url::Url::parse(authorize_url).map_err(|_| StatusCode::NOT_FOUND)?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", "openid");
    if params.get("prompt").map(String::as_str) == Some("none") {
        url.query_pairs_mut().append_pair("prompt", "none");
    }
    Ok(axum::response::Redirect::temporary(url.as_str()))
}

/// Обработчик возврата из Keycloak: обменять code на токен и вернуть его
/// веб-клиенту (redirect на front_url с токеном в фрагменте URL). В silent-флоу
/// (параметр `silent=1`) вместо редиректа отвечает HTML-страницей, которая
/// передаёт токен родителю через postMessage (см. silent_auth_response).
async fn auth_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<axum::response::Response, StatusCode> {
    let sso = state
        .config
        .sso
        .as_ref()
        .filter(|s| s.enabled)
        .ok_or(StatusCode::NOT_FOUND)?;
    let silent = params.contains_key("silent");
    // prompt=none без активной SSO-сессии: Keycloak возвращает error
    // (login_required) вместо кода — в silent-флоу сообщаем родителю, в
    // обычном — 400.
    if params.contains_key("error") {
        if silent {
            return Ok(silent_auth_response(&state, None).into_response());
        }
        return Err(StatusCode::BAD_REQUEST);
    }
    let code = params.get("code").ok_or(StatusCode::BAD_REQUEST)?;
    let token_url = sso.token_url.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    let client_id = sso.client_id.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    let client_secret = sso.client_secret.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    // Keycloak не эхоит redirect_uri в колбэке — вычисляем из Host, чтобы обмен
    // code→token прошёл с тем же значением, что и на authorize-шаге.
    let redirect_uri = params
        .get("redirect_uri")
        .cloned()
        .or_else(|| sso_redirect_uri(&headers))
        .ok_or(StatusCode::BAD_REQUEST)?;

    let client = reqwest::Client::new();
    let resp = client
        .post(token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &redirect_uri),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ])
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let json: serde_json::Value = resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let token = json
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or(StatusCode::BAD_GATEWAY)?;
    // Refresh-токен живёт дольше access-токена: веб-клиент обновляет им токен
    // молча (`/auth/refresh`), не завися от SSO-куки Keycloak в iframe.
    let refresh_token = json.get("refresh_token").and_then(|v| v.as_str());

    // Cookie ставим в обоих случаях — прямые api.localhost-клиенты тоже свежие.
    let cookie = format!("aga_token={token}; Path=/; HttpOnly; SameSite=Lax");
    if silent {
        let mut resp = silent_auth_response(&state, Some(token)).into_response();
        resp.headers_mut().insert(
            axum::http::header::SET_COOKIE,
            cookie
                .parse()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        );
        return Ok(resp);
    }

    // Веб-клиент (front/) получает токены фрагментом URL — фрагмент не уходит
    // на сервер и не остаётся в логах. Refresh-токен — вторым параметром,
    // чтобы обновлять access-токен без повторного входа.
    let fragment = match refresh_token {
        Some(rt) => format!("#token={token}&refresh={rt}"),
        None => format!("#token={token}"),
    };
    let mut resp = axum::response::Redirect::temporary(&format!("{}{fragment}", state.front_url))
        .into_response();
    resp.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        cookie
            .parse()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );
    Ok(resp)
}

/// HTML-ответ для silent-обновления токена: еслиrame (наш же веб-клиент)
/// получает токен и передаёт его родителю через postMessage. Ошибка (нет
/// активной SSO-сессии) — сообщение `aga_sso_error` без токена.
fn silent_auth_response(state: &AppState, token: Option<&str>) -> axum::response::Response {
    let payload = match token {
        Some(t) => serde_json::json!({ "type": "aga_sso_token", "token": t }),
        None => serde_json::json!({ "type": "aga_sso_error" }),
    };
    let html = format!(
        "<!DOCTYPE html><html><body><script>window.parent.postMessage({payload}, {origin});</script></body></html>",
        payload = serde_json::to_string(&payload).unwrap(),
        origin = serde_json::to_string(&state.front_url).unwrap(),
    );
    let mut resp = axum::response::Response::new(axum::body::Body::from(html));
    resp.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::header::HeaderValue::from_static("text/html; charset=utf-8"),
    );
    resp
}

/// Выход: сбрасываем cookie `aga_token` (HttpOnly — JS его сам не удалит) и,
/// если задан end-session эндпоинт Keycloak, редиректим туда (завершение
/// SSO-сессии, возврат на фронт), иначе — просто на фронт. Веб-клиент перед
/// этим удаляет токен из localStorage.
async fn auth_logout(
    State(state): State<AppState>,
) -> Result<axum::response::Response, StatusCode> {
    let sso = state.config.sso.as_ref().filter(|s| s.enabled);
    let target = match sso {
        Some(s) => match s.end_session_url.as_deref() {
            Some(end) => {
                let mut url = url::Url::parse(end).map_err(|_| StatusCode::NOT_FOUND)?;
                url.query_pairs_mut()
                    .append_pair("client_id", s.client_id.as_deref().unwrap_or_default())
                    .append_pair("post_logout_redirect_uri", &state.front_url);
                url.to_string()
            }
            None => state.front_url.clone(),
        },
        None => state.front_url.clone(),
    };
    let mut resp = axum::response::Redirect::temporary(&target).into_response();
    resp.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        "aga_token=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"
            .parse()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );
    Ok(resp)
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// Молча обновить access-токен: веб-клиент шлёт refresh-токен, ядро меняет его
/// у Keycloak (`grant_type=refresh_token`) и возвращает свежие токены JSON'ом
/// плюс кладёт access-токен в cookie `aga_token` (прямые api.localhost-клиенты).
/// В отличие от silent-iframe, браузерные куки и их кросс-сайтовая блокировка
/// не участвуют. Недействительный/истёкший refresh-токен — 401.
async fn auth_refresh(
    State(state): State<AppState>,
    Json(payload): Json<RefreshRequest>,
) -> Result<axum::response::Response, StatusCode> {
    let sso = state
        .config
        .sso
        .as_ref()
        .filter(|s| s.enabled)
        .ok_or(StatusCode::NOT_FOUND)?;
    let token_url = sso.token_url.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    let client_id = sso.client_id.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    let client_secret = sso.client_secret.as_deref().ok_or(StatusCode::NOT_FOUND)?;

    let client = reqwest::Client::new();
    let resp = client
        .post(token_url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &payload.refresh_token),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ])
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !resp.status().is_success() {
        // Keycloak отвечает 400 с error=invalid_grant, когда refresh-токен
        // недействителен или уже использован — для клиента это «войдите заново».
        return Err(StatusCode::UNAUTHORIZED);
    }
    let json: serde_json::Value = resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let token = json
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or(StatusCode::BAD_GATEWAY)?;
    // Keycloak ротирует refresh-токен — возвращаем новый, чтобы клиент сохранил.
    let new_refresh = json
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .unwrap_or(&payload.refresh_token);

    let body = Json(serde_json::json!({
        "access_token": token,
        "refresh_token": new_refresh,
        "expires_in": json.get("expires_in"),
    }))
    .into_response();
    let mut resp = body;
    let cookie = format!("aga_token={token}; Path=/; HttpOnly; SameSite=Lax");
    resp.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        cookie
            .parse()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );
    Ok(resp)
}

// === Модель чата ===

async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::chat::ChatUser>>, StatusCode> {
    current_user(&state, &headers).await?;
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
    headers: HeaderMap,
) -> Result<Json<crate::chat::ChatUser>, StatusCode> {
    current_user(&state, &headers).await?;
    match state.chat_store.get_user(id).await {
        Ok(Some(u)) => Ok(Json(u)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Текущий пользователь (по токену/куке). Веб-клиент берёт его из `/users/me`
/// для отображения в шапке; он же — проба доступа (401 = нужен вход).
async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::chat::ChatUser>, StatusCode> {
    let user_id = current_user(&state, &headers).await?;
    state
        .chat_store
        .get_user(user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

#[derive(Serialize)]
struct ConnectionJwtResponse {
    token: String,
}

/// Connection-JWT для веб-клиента Centrifuge. Только для аутентифицированных
/// (current_user → 401 без токена); Centrifugo не настроен — 404, клиент
/// деградирует молча.
async fn connection_jwt(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ConnectionJwtResponse>, StatusCode> {
    let user_id = current_user(&state, &headers).await?;
    if !state.centrifuge.is_configured() {
        return Err(StatusCode::NOT_FOUND);
    }
    let token = state
        .centrifuge
        .connection_jwt(user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ConnectionJwtResponse { token }))
}

#[derive(Deserialize)]
pub struct CreateChatRequest {
    #[serde(default)]
    pub title: Option<String>,
}

async fn create_chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateChatRequest>,
) -> Result<Json<Chat>, StatusCode> {
    let user_id = current_user(&state, &headers).await?;
    match state
        .chat_store
        .create_chat(None, payload.title.as_deref(), user_id)
        .await
    {
        Ok(chat) => {
            publish_lifecycle(&state, "chat_created", chat.id).await;
            Ok(Json(chat))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn list_chats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Chat>>, StatusCode> {
    let user_id = current_user(&state, &headers).await?;
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
    let user_id = current_user(&state, &headers).await?;
    if !can_read(&state, id, user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }
    match chat_detail(&state, id).await {
        Some(detail) => Ok(Json(detail)),
        None => Err(StatusCode::NOT_FOUND),
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

async fn close_chat(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
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
        Err(SessionError::NotFound) => Err(StatusCode::NOT_FOUND),
        Err(SessionError::Forbidden) => Err(StatusCode::FORBIDDEN),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
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

async fn send_message(
    Path(chat_id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, StatusCode> {
    let user_id = current_user(&state, &headers).await?;
    if !can_write(&state, chat_id, user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

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
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        // Чат не знает про агентов: только публикует событие в каналы. Подписчик
        // (`aga agent`) сам решает, запускать ли агента по привязке к автору.
        state
            .centrifuge
            .publish_message(chat_id, message.id, user_id)
            .await;

        Ok(Json(SendMessageResponse { message, invited }))
    } else {
        Err(StatusCode::BAD_REQUEST)
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
async fn start_message_thread(
    Path(message_id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<StartThreadRequest>,
) -> Result<Json<StartThreadResponse>, StatusCode> {
    let user_id = current_user(&state, &headers).await?;
    let Some(origin) = state
        .chat_store
        .get_message(message_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Err(StatusCode::NOT_FOUND);
    };
    if !can_write(&state, origin.chat_id, user_id).await {
        return Err(StatusCode::FORBIDDEN);
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
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Err(StatusCode::BAD_REQUEST);
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
async fn post_message_to_parent(
    Path(message_id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Message>, StatusCode> {
    let user_id = current_user(&state, &headers).await?;
    let Some(original) = state
        .chat_store
        .get_message(message_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Err(StatusCode::NOT_FOUND);
    };
    let Some(thread) = state
        .chat_store
        .get_chat(original.chat_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Err(StatusCode::NOT_FOUND);
    };
    let Some(parent_id) = thread.parent_id else {
        return Err(StatusCode::BAD_REQUEST);
    };
    if !can_write(&state, parent_id, user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }
    let Some(message) = state
        .chat_store
        .post_to_parent(message_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Err(StatusCode::BAD_REQUEST);
    };
    state
        .centrifuge
        .publish_message(parent_id, message.id, message.author_id)
        .await;
    Ok(Json(message))
}

async fn list_chat_messages(
    Path(chat_id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Message>>, StatusCode> {
    let user_id = current_user(&state, &headers).await?;
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
    let user_id = current_user(&state, &headers).await?;
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
    let user_id = current_user(&state, &headers).await?;
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
        Ok(Some(msg)) => {
            state
                .centrifuge
                .publish_message(msg.chat_id, msg.id, msg.author_id)
                .await;
            Ok(Json(msg))
        }
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
    headers: HeaderMap,
) -> Result<Json<Vec<crate::chat::Workstation>>, StatusCode> {
    current_user(&state, &headers).await?;
    state
        .chat_store
        .list_workstations()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Deserialize)]
pub struct CreateWorkstationRequest {
    #[serde(default)]
    pub name: Option<String>,
    /// Имя k8s-Secret из кластера, монтируемого в под при подъёме.
    #[serde(default)]
    pub secret: Option<String>,
}

/// Создание воркстейшна — только для суперпользователя (админ внешний,
/// интерфейс станции не создаёт и не удаляет). Участники получают 403.
/// Станция поднимается свободной и пустой: проект разворачивается на ней
/// при открытии сессии (`POST /workstations/:id/session`).
async fn create_workstation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateWorkstationRequest>,
) -> Result<Json<crate::chat::Workstation>, StatusCode> {
    let user_id = current_user(&state, &headers).await?;
    if !state
        .chat_store
        .is_super_user(user_id)
        .await
        .unwrap_or(false)
    {
        return Err(StatusCode::FORBIDDEN);
    }

    let name = payload.name.unwrap_or_else(|| "ws".to_string());

    // SSH-ключ aga (env): прокидывается в воркстейшн автоматически. Явный
    // `secret` из запроса (имя k8s-Secret) имеет приоритет; иначе при
    // заданном ключе используется общий `aga-ssh`.
    let ssh_key = crate::ssh_key::private_key_from_env();
    let effective_secret = match (payload.secret.clone(), ssh_key.as_ref()) {
        (Some(secret), _) => Some(secret),
        (None, Some(_)) => Some(crate::ssh_key::SSH_SECRET_NAME.to_string()),
        (None, None) => None,
    };

    // Воркстейшн — под в Kubernetes: сначала запись, потом сам под.
    let ws = state
        .chat_store
        .create_workstation(&name, effective_secret.as_deref())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Станция пустая: GIT_URL не задан — entrypoint инициализирует пустое
    // git-репо в /work/project, код проекта развернёт открытие сессии.
    let pod_name = Cluster::pod_name(ws.id);
    let branch = Cluster::branch_name(ws.id);

    // Ключ до подъёма станции: в k8s — Secret в кластере (монтируется в под
    // манифестом, поэтому создаётся до пода). В dev-режиме (docker) ключ
    // инжектится после `create_pod` — контейнер должен существовать.
    if let (Some(key), crate::cluster::Backend::K8s) = (ssh_key.as_ref(), state.cluster.backend) {
        if let Err(e) = state
            .cluster
            .ensure_ssh_secret(crate::ssh_key::SSH_SECRET_NAME, key)
            .await
        {
            tracing::error!(
                "failed to ensure ssh secret {}: {e}",
                crate::ssh_key::SSH_SECRET_NAME
            );
            let _ = state
                .chat_store
                .set_workstation_state(ws.id, "failed")
                .await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    if let Err(e) = state
        .cluster
        .create_pod(&pod_name, "", &branch, ws.secret.as_deref())
        .await
    {
        tracing::error!("failed to create workstation pod {pod_name}: {e}");
        let _ = state
            .chat_store
            .set_workstation_state(ws.id, "failed")
            .await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // dev-режим: ключ в ~/.ssh существующего контейнера (созданного выше или
    // переиспользованного из compose-стенда). Сбой инъекции не роняет станцию.
    if let (Some(key), crate::cluster::Backend::Docker) = (ssh_key.as_ref(), state.cluster.backend)
    {
        if let Err(e) = state.cluster.inject_ssh_key(&pod_name, key).await {
            tracing::error!("failed to inject ssh key into {pod_name}: {e}");
        }
    }

    // Под уже создан; готовность (Running + пустое git-репо) подтягиваем
    // ожиданием — под тянет образ, это занимает время. Проект на станции
    // появится при открытии сессии.
    if state.cluster.wait_ready(&pod_name).await.unwrap_or(false) {
        let _ = state.chat_store.set_workstation_state(ws.id, "ready").await;
    }

    state
        .chat_store
        .get_workstation(ws.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
        .map(Json)
}

async fn delete_workstation(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    let user_id = current_user(&state, &headers).await?;
    if !state
        .chat_store
        .is_super_user(user_id)
        .await
        .unwrap_or(false)
    {
        return Err(StatusCode::FORBIDDEN);
    }
    // Сначала под, потом запись: если кластер не отдал под — состояние не
    // трогаем, пользователь повторит.
    if let Err(e) = state.cluster.delete_pod(&Cluster::pod_name(id)).await {
        tracing::error!("failed to delete workstation pod ws-{id}: {e}");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    match state.chat_store.delete_workstation(id).await {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Отпустить воркстейшн — только суперпользователь. Свободная станция (без
/// открытой сессии): файлы проекта очищаются до пустого git-репо; сам ws
/// (под/сервис) не пересоздаётся.
async fn release_workstation(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::chat::Workstation>, StatusCode> {
    let user_id = current_user(&state, &headers).await?;
    if !state
        .chat_store
        .is_super_user(user_id)
        .await
        .unwrap_or(false)
    {
        return Err(StatusCode::FORBIDDEN);
    }
    let ws = state
        .chat_store
        .release_workstation(id)
        .await
        .map_err(|e| match e {
            SessionError::NotFound => StatusCode::NOT_FOUND,
            SessionError::WorkstationBusy => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })?;
    let executor = crate::workstation::executor_for_workstation(Some(id), &state.cluster);
    if let Err(e) = crate::ws_ops::release_workspace(&executor).await {
        tracing::warn!("failed to clear workspace on released ws-{id}: {e}");
    }
    Ok(Json(ws))
}

/// Отметить упавший воркстейшн как недоступный (инициирует восстановление).
async fn mark_workstation_down(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    let user_id = current_user(&state, &headers).await?;
    if !state
        .chat_store
        .is_super_user(user_id)
        .await
        .unwrap_or(false)
    {
        return Err(StatusCode::FORBIDDEN);
    }
    if state
        .chat_store
        .get_workstation(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .is_none()
    {
        return Err(StatusCode::NOT_FOUND);
    }
    state
        .chat_store
        .mark_workstation_down(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

// === Просмотр содержимого проекта в воркстейшне ===

/// Относительный путь внутри проекта; пустой — корень.
#[derive(Deserialize)]
pub struct ProjectFilePathQuery {
    #[serde(default)]
    pub path: String,
}

/// Дерево папок и файлов проекта воркстейшна. Читается напрямую из
/// под/контейнера (exec find) и доступно любому вошедшему участнику —
/// персональной видимости нет (см. `can_read`).
async fn workstation_tree(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<ProjectFilePathQuery>,
) -> Result<Json<crate::project_files::Tree>, StatusCode> {
    current_user(&state, &headers).await?;
    if state
        .chat_store
        .get_workstation(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .is_none()
    {
        return Err(StatusCode::NOT_FOUND);
    }
    let executor = crate::workstation::executor_for_workstation(Some(id), &state.cluster);
    let tree =
        crate::project_files::tree(&executor, crate::project_files::PROJECT_ROOT, &params.path)
            .await
            .map_err(map_file_error)?;
    Ok(Json(tree))
}

/// Содержимое файла: текст (text/plain, подсветка на фронте) или байты
/// с MIME (картинки/видео/аудио). Только чтение — записывать нельзя.
async fn workstation_file(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<ProjectFilePathQuery>,
) -> Result<Response, StatusCode> {
    current_user(&state, &headers).await?;
    if state
        .chat_store
        .get_workstation(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .is_none()
    {
        return Err(StatusCode::NOT_FOUND);
    }
    let executor = crate::workstation::executor_for_workstation(Some(id), &state.cluster);
    let content =
        crate::project_files::read(&executor, crate::project_files::PROJECT_ROOT, &params.path)
            .await
            .map_err(map_file_error)?;
    Ok(file_response(content))
}

fn map_file_error(e: crate::project_files::FileError) -> StatusCode {
    match e {
        crate::project_files::FileError::InvalidPath(_) => StatusCode::BAD_REQUEST,
        crate::project_files::FileError::NotFound => StatusCode::NOT_FOUND,
        crate::project_files::FileError::Exec(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Изменения проекта воркстейшна от начала ветки (страница Changes). Дифф
/// берётся из git внутри воркстейшна; доступен любому вошедшему участнику,
/// как и просмотр содержимого. Только чтение: коммиты и push не выполняются.
async fn workstation_changes(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::git_changes::ChangesSummary>, StatusCode> {
    current_user(&state, &headers).await?;
    if state
        .chat_store
        .get_workstation(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .is_none()
    {
        return Err(StatusCode::NOT_FOUND);
    }
    let executor = crate::workstation::executor_for_workstation(Some(id), &state.cluster);
    let summary = crate::git_changes::changes(&executor, crate::project_files::PROJECT_ROOT)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(summary))
}

/// Публичный SSH-ключ aga (общий на инстанс) для страницы «Настройки».
/// Приватный ключ задаёт админ в env `AGA_SSH_PRIVATE_KEY`; здесь отдаётся
/// только публичный (не секрет) — для добавления в deploy-ключи репозитория.
#[derive(Serialize)]
pub struct SshKeyInfo {
    /// Есть ли настроенный ключ (env задан).
    configured: bool,
    /// Публичный ключ в OpenSSH-формате (`ssh-ed25519 AAAA...`).
    public_key: Option<String>,
    /// SHA256-fingerprint (`SHA256:...`).
    fingerprint: Option<String>,
}

async fn get_ssh_key(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SshKeyInfo>, StatusCode> {
    current_user(&state, &headers).await?;
    let Some(private_key) = crate::ssh_key::private_key_from_env() else {
        return Ok(Json(SshKeyInfo {
            configured: false,
            public_key: None,
            fingerprint: None,
        }));
    };
    match crate::ssh_key::derive_public_key(&private_key) {
        Ok((public_key, fingerprint)) => Ok(Json(SshKeyInfo {
            configured: true,
            public_key: Some(public_key),
            fingerprint: Some(fingerprint),
        })),
        Err(e) => {
            tracing::error!("invalid {}: {e}", crate::ssh_key::SSH_PRIVATE_KEY_ENV);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn file_response(content: crate::project_files::FileContent) -> Response {
    if content.mime.starts_with("text/") {
        let text = String::from_utf8_lossy(&content.bytes).to_string();
        let mut resp = (StatusCode::OK, text).into_response();
        resp.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        resp
    } else {
        let mut resp = Response::new(Body::from(content.bytes));
        if let Ok(hv) = HeaderValue::from_str(&content.mime) {
            resp.headers_mut().insert(CONTENT_TYPE, hv);
        }
        resp
    }
}

#[derive(Deserialize)]
pub struct OpenWorkstationSessionRequest {
    pub project_id: i64,
    #[serde(default)]
    pub title: Option<String>,
}

/// Активная сессия воркстейшна (или null).
async fn get_workstation_session(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Option<Chat>>, StatusCode> {
    current_user(&state, &headers).await?;
    let chat_id = state
        .chat_store
        .active_session_id(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some(chat_id) = chat_id else {
        return Ok(Json(None));
    };
    let chat = state
        .chat_store
        .get_chat(chat_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(chat))
}

/// Открыть сессию на воркстейшне: любой участник, воркстейшн готов,
/// открытой сессии на нём нет. Станция обслуживает сессии: проект задаётся
/// здесь и разворачивается в `/work/project` (код проекта станция до этого
/// не знает). Закрытие сессии освобождает воркстейшн.
async fn open_workstation_session(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<OpenWorkstationSessionRequest>,
) -> Result<Json<Chat>, StatusCode> {
    let user_id = current_user(&state, &headers).await?;
    let git_url = match state.trace_store.get_project(payload.project_id).await {
        Ok(Some(p)) => p.git_url,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let chat = match state
        .chat_store
        .open_workstation_session(id, payload.project_id, payload.title.as_deref(), user_id)
        .await
    {
        Ok(chat) => chat,
        Err(SessionError::NotFound) => return Err(StatusCode::NOT_FOUND),
        Err(SessionError::WorkstationNotReady) => return Err(StatusCode::CONFLICT),
        Err(SessionError::WorkstationBusy) => return Err(StatusCode::CONFLICT),
        Err(SessionError::Forbidden) => return Err(StatusCode::FORBIDDEN),
        Err(SessionError::Db(_)) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let executor = crate::workstation::executor_for_workstation(Some(id), &state.cluster);
    let branch = Cluster::branch_name(id);
    // SSH-ключ aga для git+ssh-клона: в k8s-поде ключ приходит из Secret при
    // создании, в dev (docker) контейнеры ws поднимаются заранее и
    // переиспользуются — ключ инжектится перед клонированием проекта.
    if let (Some(key), crate::cluster::Backend::Docker) = (
        crate::ssh_key::private_key_from_env().as_ref(),
        state.cluster.backend,
    ) {
        if let Err(e) = state
            .cluster
            .inject_ssh_key(&Cluster::pod_name(id), key)
            .await
        {
            tracing::warn!("failed to inject ssh key into ws-{id}: {e}");
        }
    }
    // Развернуть код проекта на станции — лучшая попытка: в dev-стенде git_url
    // может быть плейсхолдером (станция работает с примонтированной копией и
    // проект наполняет агент), поэтому сбой exec не откатывает сессию, а
    // только логируется.
    if let Err(e) = crate::ws_ops::replace_project(&executor, &git_url, &branch).await {
        tracing::warn!("failed to deploy project {git_url} on ws-{id}: {e}");
    }
    // Ручное восстановление после падения: если открытая сессия — продолжение
    // прерванной на упавшей станции, восстанавливаем файлы проекта из её ветки.
    // Сбой восстановления сессию не роняет — станция остаётся на чистом клоне,
    // ошибка уходит в лог.
    if let Ok(Some(prev)) = state.chat_store.continues_session_id(chat.id).await {
        if let Ok(Some(prev_branch)) = state.chat_store.session_branch(prev).await {
            if let Err(e) = crate::ws_ops::restore_workspace(&executor, &prev_branch).await {
                tracing::warn!("failed to restore session from ws into ws-{id}: {e}");
            }
        }
    }
    publish_lifecycle(&state, "session_opened", chat.id).await;
    Ok(Json(chat))
}

// === Permission helpers ===

/// Читать чат можно всем: персональной видимости нет, участники видят
/// все сессии всех проектов.
async fn can_read(_state: &AppState, _chat_id: i64, _user_id: i64) -> bool {
    true
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

/// Скрытая часть сообщения из сокращений: каждое слово `/имя` в тексте
/// добавляет текст привязанного сокращения (одно имя — один раз). Неизвестные
/// имена и пустой текст пропускаются. Скрытая часть — заметка: в контекст
/// агентов (`build_context`) она не входит.
/// Событие жизненного цикла чата (создание чата, открытие/закрытие сессии) —
/// в общий канал, best-effort: подписчик (веб-клиент) по нему обновляет список.
async fn publish_lifecycle(state: &AppState, action: &str, chat_id: i64) {
    let channel = state.centrifuge.channel();
    state
        .centrifuge
        .publish(
            &channel,
            crate::centrifuge::lifecycle_payload(action, chat_id),
        )
        .await;
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http::{Request, StatusCode};
    use tower::ServiceExt;

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

    #[tokio::test]
    async fn participant_sees_all_projects_via_api() {
        let (state, file) = test_state(true).await;
        let headers = auth_headers("alice", &["participant"]);
        let (_, _) = post_json(
            "/projects",
            &headers,
            state.clone(),
            serde_json::json!({"git_url": "https://example.com/a.git"}),
        )
        .await;
        let (status, _) = get("/projects", &headers, state.clone()).await;
        assert_eq!(status, StatusCode::OK);
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn participant_creates_project_visible_to_others() {
        let (state, file) = test_state(true).await;
        let alice = auth_headers("alice", &["participant"]);
        let (status, _) = post_json(
            "/projects",
            &alice,
            state.clone(),
            serde_json::json!({"git_url": "https://example.com/a.git"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // Проект виден другому участнику.
        let bob = auth_headers("bob", &["participant"]);
        let (status, body) = get("/projects", &bob, state.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("example.com/a.git"));
        cleanup(&file).await;
    }

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
    async fn login_redirects_to_keycloak_when_configured() {
        let (state, file) = test_state(true).await;
        // Проверяем /auth/login: без SSO-конфигурации эндпоинтов — 404.
        let _headers = HeaderMap::new();
        let router = create_router(state.clone());
        let resp = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/auth/login")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn login_redirects_with_absolute_redirect_uri_from_host() {
        let (mut state, file) = test_state(false).await;
        state.config.sso = Some(crate::config::SsoConfig {
            enabled: true,
            jwks_url: None,
            authorize_url: Some(
                "http://auth.localhost/realms/aga/protocol/openid-connect/auth".into(),
            ),
            token_url: None,
            end_session_url: None,
            client_id: Some("aga".into()),
            client_secret: None,
        });
        let router = create_router(state);
        let resp = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/auth/login")
                    .header("Host", "dev.localhost")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
        let location = resp
            .headers()
            .get(axum::http::header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        let url = url::Url::parse(location).unwrap();
        let redirect_uri = url
            .query_pairs()
            .find(|(k, _)| k == "redirect_uri")
            .map(|(_, v)| v.into_owned())
            .unwrap();
        // Абсолютный redirect_uri на хосте SPA, иначе Keycloak после входа
        // вернёт браузер на свой хост (auth.localhost) и будет 404.
        assert_eq!(redirect_uri, "http://dev.localhost/auth/callback");
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn login_silent_echoes_silent_into_redirect_uri_and_prompt_none() {
        let (mut state, file) = test_state(false).await;
        state.config.sso = Some(crate::config::SsoConfig {
            enabled: true,
            jwks_url: None,
            authorize_url: Some(
                "http://auth.localhost/realms/aga/protocol/openid-connect/auth".into(),
            ),
            token_url: None,
            end_session_url: None,
            client_id: Some("aga".into()),
            client_secret: None,
        });
        let router = create_router(state);
        let resp = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/auth/login?prompt=none&silent=1")
                    .header("Host", "dev.localhost")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
        let location = resp
            .headers()
            .get(axum::http::header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        let url = url::Url::parse(location).unwrap();
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        // redirect_uri эхоит silent=1, чтобы колбэк ответил postMessage-страницей.
        assert!(pairs.contains(&(
            "redirect_uri".into(),
            "http://dev.localhost/auth/callback?silent=1".into()
        )));
        // prompt=none: Keycloak без формы входа.
        assert!(pairs.contains(&("prompt".into(), "none".into())));
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn callback_silent_error_returns_post_message_page() {
        let (mut state, file) = test_state(false).await;
        state.config.sso = Some(crate::config::SsoConfig {
            enabled: true,
            jwks_url: None,
            authorize_url: None,
            token_url: None,
            end_session_url: None,
            client_id: Some("aga".into()),
            client_secret: None,
        });
        let router = create_router(state);
        let resp = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/auth/callback?silent=1&error=login_required")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let content_type = resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(content_type.starts_with("text/html"));
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("aga_sso_error"), "html: {html}");
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn callback_with_error_returns_bad_request_outside_silent_flow() {
        let (mut state, file) = test_state(false).await;
        state.config.sso = Some(crate::config::SsoConfig {
            enabled: true,
            jwks_url: None,
            authorize_url: None,
            token_url: None,
            end_session_url: None,
            client_id: Some("aga".into()),
            client_secret: None,
        });
        let router = create_router(state);
        let resp = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/auth/callback?error=login_required")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
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
    async fn logout_clears_cookie_and_redirects_to_keycloak_end_session() {
        let (mut state, file) = test_state(false).await;
        state.config.sso = Some(crate::config::SsoConfig {
            enabled: true,
            jwks_url: None,
            authorize_url: None,
            token_url: None,
            end_session_url: Some(
                "http://auth.localhost/realms/aga/protocol/openid-connect/logout".into(),
            ),
            client_id: Some("aga".into()),
            client_secret: None,
        });
        let router = create_router(state);
        let resp = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/auth/logout")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
        // Cookie aga_token сброшен (HttpOnly-куку может стереть только сервер).
        let set_cookie = resp
            .headers()
            .get(axum::http::header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(set_cookie.starts_with("aga_token="));
        assert!(set_cookie.contains("Max-Age=0"));
        // Редирект на end-session Keycloak с возвратом на фронт.
        let location = resp
            .headers()
            .get(axum::http::header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        let url = url::Url::parse(location).unwrap();
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(
            url.host_str(),
            Some("auth.localhost"),
            "end_session: {location}"
        );
        assert!(pairs.contains(&("client_id".into(), "aga".into())));
        assert!(pairs.contains(&(
            "post_logout_redirect_uri".into(),
            "http://dev.localhost".into()
        )));
        cleanup(&file).await;
    }

    /// Мок token-эндпоинта Keycloak на случайном порту: успешный обмен
    /// (свежие токены) или 400 invalid_grant.
    async fn mock_token_server(success: bool) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/token",
            axum::routing::post(move |_: axum::extract::Request| async move {
                if success {
                    (
                        axum::http::StatusCode::OK,
                        axum::Json(serde_json::json!({
                            "access_token": "fresh.access.token",
                            "refresh_token": "fresh.refresh.token",
                            "expires_in": 300,
                        })),
                    )
                } else {
                    (
                        axum::http::StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({ "error": "invalid_grant" })),
                    )
                }
            }),
        );
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}/token"), handle)
    }

    fn sso_with_token_url(token_url: String) -> crate::config::SsoConfig {
        crate::config::SsoConfig {
            enabled: true,
            jwks_url: None,
            authorize_url: None,
            token_url: Some(token_url),
            end_session_url: None,
            client_id: Some("aga".into()),
            client_secret: Some("aga-secret".into()),
        }
    }

    #[tokio::test]
    async fn refresh_exchanges_token_and_sets_cookie() {
        let (mut state, file) = test_state(false).await;
        let (token_url, _server) = mock_token_server(true).await;
        state.config.sso = Some(sso_with_token_url(token_url));
        let router = create_router(state);
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/refresh")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"refresh_token":"stale.refresh.token"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let set_cookie = resp
            .headers()
            .get(axum::http::header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(set_cookie.starts_with("aga_token="));
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["access_token"], "fresh.access.token");
        assert_eq!(json["refresh_token"], "fresh.refresh.token");
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn refresh_rejects_invalid_token() {
        let (mut state, file) = test_state(false).await;
        let (token_url, _server) = mock_token_server(false).await;
        state.config.sso = Some(sso_with_token_url(token_url));
        let router = create_router(state);
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/refresh")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"refresh_token":"stale.refresh.token"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn refresh_requires_sso_config() {
        let (state, file) = test_state(false).await;
        let router = create_router(state);
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/refresh")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"refresh_token":"x"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        cleanup(&file).await;
    }

    fn agent_json(name: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "description": format!("Правила {name}"),
            "tools": ["git", "make"],
            "max_iterations": 3,
            "llm_id": null,
            "parent": null,
            "skills": [],
            "commands": []
        })
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
            "commands": [],
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
    async fn created_llm_connection_listed_via_api() {
        let (state, file) = test_state(true).await;
        let headers = auth_headers("alice", &["participant"]);
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
        assert!(body.contains("secret-key"));
        assert!(body.contains("qwen3:0.6b"));
        // Созданное подключение видно в списке: название, url, ключ и модель.
        let (status, body) = get("/llms", &headers, state.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("ollama"));
        assert!(body.contains("http://llm:11434/v1"));
        assert!(body.contains("secret-key"));
        assert!(body.contains("qwen3:0.6b"));
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn updated_llm_connection_changes_url_and_key_via_api() {
        let (state, file) = test_state(true).await;
        let headers = auth_headers("alice", &["participant"]);
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
        assert!(body.contains("new-key"));
        assert!(body.contains("qwen3:1b"));
        assert!(!body.contains("http://old/v1"));
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn deleted_llm_connection_disappears_from_list_via_api() {
        let (state, file) = test_state(true).await;
        let headers = auth_headers("alice", &["participant"]);
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
        let headers = auth_headers("alice", &["participant"]);
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

    #[tokio::test]
    async fn attached_agent_set_appears_on_project() {
        let (state, file) = test_state(true).await;
        let headers = auth_headers("alice", &["participant"]);
        let project_id = state
            .trace_store
            .upsert_project("https://example.com/a.git")
            .await
            .unwrap();
        let set_id = state
            .trace_store
            .create_agent_set(
                "ops",
                &[crate::trace::AgentSpec {
                    name: "dev".to_string(),
                    description: "Правила разработчика".to_string(),
                    tools: vec!["git".to_string()],
                    max_iterations: 3,
                    llm_id: None,
                    parent: None,
                    skills: vec![],
                    commands: vec![],

                    listen_user_id: None,
                }],
            )
            .await
            .unwrap();
        let (status, _) = post_json(
            &format!("/projects/{project_id}/agent-set"),
            &headers,
            state.clone(),
            serde_json::json!({ "agent_set_id": set_id }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, body) = get("/projects", &headers, state.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("dev"));
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn replacing_agent_set_changes_project_agents_via_api() {
        let (state, file) = test_state(true).await;
        let headers = auth_headers("alice", &["participant"]);
        let project_id = state
            .trace_store
            .upsert_project("https://example.com/r.git")
            .await
            .unwrap();
        let set_a = state
            .trace_store
            .create_agent_set(
                "set-a",
                &[crate::trace::AgentSpec {
                    name: "dev-a".to_string(),
                    description: "a".to_string(),
                    tools: vec![],
                    max_iterations: 1,
                    llm_id: None,
                    parent: None,
                    skills: vec![],
                    commands: vec![],

                    listen_user_id: None,
                }],
            )
            .await
            .unwrap();
        let set_b = state
            .trace_store
            .create_agent_set(
                "set-b",
                &[crate::trace::AgentSpec {
                    name: "dev-b".to_string(),
                    description: "b".to_string(),
                    tools: vec![],
                    max_iterations: 1,
                    llm_id: None,
                    parent: None,
                    skills: vec![],
                    commands: vec![],

                    listen_user_id: None,
                }],
            )
            .await
            .unwrap();
        let (s, _) = post_json(
            &format!("/projects/{project_id}/agent-set"),
            &headers,
            state.clone(),
            serde_json::json!({ "agent_set_id": set_a }),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let (s, _) = post_json(
            &format!("/projects/{project_id}/agent-set"),
            &headers,
            state.clone(),
            serde_json::json!({ "agent_set_id": set_b }),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let (_, body) = get(
            &format!("/projects/{project_id}/agent-set"),
            &headers,
            state.clone(),
        )
        .await;
        assert!(body.contains("dev-b"));
        assert!(!body.contains("dev-a"));
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn agent_set_detail_includes_territory_skills_commands_and_tools() {
        let (state, file) = test_state(true).await;
        let headers = auth_headers("alice", &["participant"]);
        // Каталог: скилл и команда с единственным содержимым.
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
        let (status, _) = post_json(
            "/commands",
            &headers,
            state.clone(),
            serde_json::json!({
                "name": "deploy",
                "content": "Выкатывать"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // Набор с деревом и данными агенту способностями (по имени, без версии).
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
                    "skills": [{"name": "review"}],
                    "commands": [{"name": "deploy"}]
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
        // Состав набора: агенты, территория каждого, данные скиллы и команды
        // (по имени, без версии), инструменты — всё в детали набора.
        assert!(body.contains("territory"));
        assert!(body.contains("folder"));
        assert!(body.contains("src"));
        assert!(body.contains("tools"));
        assert!(body.contains("git"));
        assert!(body.contains("skills"));
        assert!(body.contains("review"));
        assert!(body.contains("commands"));
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
        let (status, _) =
            delete_json(&format!("/skills/{skill_id}"), &headers, state.clone()).await;
        assert_eq!(status, StatusCode::OK);
        let (status, body) = get("/skills", &headers, state.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.contains("review2"));
        let (status, _) =
            delete_json(&format!("/skills/{skill_id}"), &headers, state.clone()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        // Удалённая запись остаётся в списке «Удалённые» (?deleted=1).
        let (status, body) = get("/skills?deleted=1", &headers, state.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("review2"));
        // Команды — тот же каталог, те же правки.
        let (status, body) = post_json(
            "/commands",
            &headers,
            state.clone(),
            serde_json::json!({
                "name": "deploy",
                "content": "Выкатывать"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let cmd_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_i64()
            .unwrap();
        let (status, _) = patch_json(
            &format!("/commands/{cmd_id}"),
            &headers,
            state.clone(),
            serde_json::json!({ "name": "deploy2" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) =
            delete_json(&format!("/commands/{cmd_id}"), &headers, state.clone()).await;
        assert_eq!(status, StatusCode::OK);
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
    async fn capability_edit_skips_rename_when_name_unchanged_and_conflicts_return_409() {
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

    fn json_get(body: &str, path: &[&str]) -> Option<serde_json::Value> {
        let mut v = serde_json::from_str::<serde_json::Value>(body).ok()?;
        for key in path {
            v = v.get(*key)?.clone();
        }
        Some(v)
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

    // === Сокращения и скрытая часть сообщения ===

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
        let context = state.chat_store.context_tail(chat_id).await.unwrap();
        assert!(context.contains("/secret вопрос"));
        assert!(!context.contains("СЕКРЕТНАЯ ЗАМЕТКА"));
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
        let (status, body) =
            get(&format!("/chats/{chat_id}/messages"), &alice, state.clone()).await;
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
}
