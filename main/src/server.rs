use axum::body::Body;
use axum::{
    extract::{Path, State},
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderMap, HeaderValue, Method, StatusCode,
    },
    response::sse::{Event, Sse},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use futures_util::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio::sync::mpsc;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::auth;
use crate::chat::{parse_command, Chat, ChatCommand, ChatStore, Message, SessionError};
use crate::cluster::Cluster;
use crate::config::Config;
use crate::reactive::ReactiveRunner;
use crate::trace::TraceStore;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub trace_store: TraceStore,
    pub chat_store: ChatStore,
    pub reactive: ReactiveRunner,
    pub cluster: Cluster,
    pub sso_verifier: Option<auth::JwtVerifier>,
    /// Origin веб-клиента (front/): CORS-источник и адрес возврата токена после SSO.
    pub front_url: String,
}

/// Текущий пользователь: без SSO — аноним-суперпользователь; с SSO —
/// участник из токена. Недействительный токен — 401.
async fn current_user(state: &AppState, headers: &HeaderMap) -> Result<i64, StatusCode> {
    auth::resolve_user(headers, &state.chat_store, state.sso_verifier.as_ref()).await
}

#[derive(Deserialize)]
pub struct HumanAnswerRequest {
    pub answer: String,
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
pub struct CreateCapabilityRequest {
    pub name: String,
    pub versions: Vec<crate::trace::CapabilityVersion>,
}

#[derive(Deserialize)]
pub struct AddCapabilityVersionRequest {
    pub version: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct RenameCapabilityRequest {
    pub name: String,
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
        .route("/human/pending", get(pending_human_requests))
        .route("/human/answer/:id", post(answer_human_request))
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
        // === Каталог способностей (скиллы и команды с версиями) ===
        .route("/skills", get(list_skills).post(create_skill))
        .route("/skills/:id", patch(rename_skill).delete(delete_skill))
        .route("/skills/:id/versions", post(add_skill_version))
        .route("/commands", get(list_commands).post(create_command))
        .route(
            "/commands/:id",
            patch(rename_command).delete(delete_command),
        )
        .route("/commands/:id/versions", post(add_command_version))
        // === SSO (Keycloak): вход веб-клиента ===
        .route("/auth/login", get(auth_login))
        .route("/auth/callback", get(auth_callback))
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
        .route(
            "/workstations/:id/session",
            get(get_workstation_session).post(open_workstation_session),
        )
        .route(
            "/workstations/:id",
            axum::routing::delete(delete_workstation),
        )
        .route("/workstations/:id/switch", post(switch_workstation))
        .route("/workstations/:id/release", post(release_workstation))
        .route("/workstations/:id/down", post(mark_workstation_down))
        // === Просмотр содержимого проекта в воркстейшне ===
        .route("/workstations/:id/tree", get(workstation_tree))
        .route("/workstations/:id/file", get(workstation_file))
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

async fn list_skills(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::trace::CapabilityItem>>, StatusCode> {
    current_user(&state, &headers).await?;
    state
        .trace_store
        .list_capabilities(crate::trace::CapabilityKind::Skill)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn create_skill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateCapabilityRequest>,
) -> Result<Json<crate::trace::CapabilityItem>, StatusCode> {
    current_user(&state, &headers).await?;
    let id = state
        .trace_store
        .create_capability(
            crate::trace::CapabilityKind::Skill,
            &payload.name,
            &payload.versions,
        )
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

async fn add_skill_version(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AddCapabilityVersionRequest>,
) -> Result<StatusCode, StatusCode> {
    current_user(&state, &headers).await?;
    state
        .trace_store
        .add_capability_version(id, &payload.version, &payload.content)
        .await
        .map(|_| StatusCode::OK)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn list_commands(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::trace::CapabilityItem>>, StatusCode> {
    current_user(&state, &headers).await?;
    state
        .trace_store
        .list_capabilities(crate::trace::CapabilityKind::Command)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn create_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateCapabilityRequest>,
) -> Result<Json<crate::trace::CapabilityItem>, StatusCode> {
    current_user(&state, &headers).await?;
    let id = state
        .trace_store
        .create_capability(
            crate::trace::CapabilityKind::Command,
            &payload.name,
            &payload.versions,
        )
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

async fn add_command_version(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AddCapabilityVersionRequest>,
) -> Result<StatusCode, StatusCode> {
    current_user(&state, &headers).await?;
    state
        .trace_store
        .add_capability_version(id, &payload.version, &payload.content)
        .await
        .map(|_| StatusCode::OK)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn rename_skill(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RenameCapabilityRequest>,
) -> Result<StatusCode, StatusCode> {
    current_user(&state, &headers).await?;
    match state.trace_store.rename_capability(id, &payload.name).await {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn delete_skill(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    current_user(&state, &headers).await?;
    match state.trace_store.delete_capability(id).await {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn rename_command(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RenameCapabilityRequest>,
) -> Result<StatusCode, StatusCode> {
    current_user(&state, &headers).await?;
    match state.trace_store.rename_capability(id, &payload.name).await {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn delete_command(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    current_user(&state, &headers).await?;
    match state.trace_store.delete_capability(id).await {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
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
async fn auth_login(
    State(state): State<AppState>,
    headers: HeaderMap,
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
    let mut url = url::Url::parse(authorize_url).map_err(|_| StatusCode::NOT_FOUND)?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", "openid");
    Ok(axum::response::Redirect::temporary(url.as_str()))
}

/// Обработчик возврата из Keycloak: обменять code на токен и вернуть его
/// веб-клиенту (redirect на front_url с токеном в фрагменте URL).
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

    // Веб-клиент (front/) получает токен фрагментом URL — фрагмент не уходит
    // на сервер и не остаётся в логах. Cookie ставим для прямых api.localhost-клиентов.
    let mut resp =
        axum::response::Redirect::temporary(&format!("{}/#token={}", state.front_url, token))
            .into_response();
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
    let user_id = current_user(&state, &headers).await?;
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
    let user_id = current_user(&state, &headers).await?;
    match state
        .chat_store
        .close_workstation_session(id, user_id)
        .await
    {
        Ok(()) => Ok(StatusCode::OK),
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
    let user_id = current_user(&state, &headers).await?;
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
        // Реактивные агенты по упоминаниям @Agent.<имя> из набора проекта.
        for name in crate::chat::mentioned_roles(&payload.body) {
            if let Ok(agent_user_id) = state.chat_store.ensure_agent_user(&name).await {
                let context = build_context(&state, chat_id)
                    .await
                    .unwrap_or_else(|| payload.body.clone());
                state
                    .reactive
                    .enqueue(chat_id, &name, agent_user_id, context);
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
    pub project_id: i64,
    #[serde(default)]
    pub name: Option<String>,
    /// Имя k8s-Secret из кластера, монтируемого в под при подъёме.
    #[serde(default)]
    pub secret: Option<String>,
}

/// Тело запроса на переключение воркстейшна на другой проект.
#[derive(Deserialize)]
pub struct SwitchWorkstationRequest {
    pub project_id: i64,
}

/// Создание воркстейшна — только для суперпользователя (админ внешний,
/// интерфейс станции не создаёт и не удаляет). Участники получают 403.
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

    // Воркстейшн — под в Kubernetes: сначала запись, потом сам под.
    let ws = state
        .chat_store
        .create_workstation(payload.project_id, &name, payload.secret.as_deref())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let git_url = match state.trace_store.get_project(payload.project_id).await {
        Ok(Some(p)) => p.git_url,
        _ => {
            let _ = state
                .chat_store
                .set_workstation_state(ws.id, "failed")
                .await;
            return Err(StatusCode::NOT_FOUND);
        }
    };

    let pod_name = Cluster::pod_name(ws.id);
    let branch = Cluster::branch_name(ws.id);

    if let Err(e) = state
        .cluster
        .create_pod(&pod_name, &git_url, &branch, ws.secret.as_deref())
        .await
    {
        tracing::error!("failed to create workstation pod {pod_name}: {e}");
        let _ = state
            .chat_store
            .set_workstation_state(ws.id, "failed")
            .await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Под уже создан; готовность (Running) подтягиваем ожиданием — под
    // тянет образ и клонирует проект, это занимает время.
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

/// Переключить воркстейшн на другой проект — только суперпользователь.
/// Свободная станция (без открытой сессии) меняет проект: `/work/project`
/// переписывается кодом нового проекта, сам ws (под/сервис) не пересоздаётся.
async fn switch_workstation(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SwitchWorkstationRequest>,
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
        .switch_workstation_project(id, payload.project_id)
        .await
        .map_err(|e| match e {
            SessionError::NotFound => StatusCode::NOT_FOUND,
            SessionError::WorkstationBusy => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })?;
    let git_url = match state.trace_store.get_project(payload.project_id).await {
        Ok(Some(p)) => p.git_url,
        _ => return Err(StatusCode::NOT_FOUND),
    };
    let executor = crate::workstation::executor_for_workstation(Some(id), &state.cluster);
    let branch = Cluster::branch_name(id);
    // Смена проекта уже зафиксирована в БД (источник истины для списка).
    // Перезапись /work/project — лучшая попытка: в dev-стенде git_url может
    // быть плейсхолдером (воркстейшн работает с примонтированной копией),
    // поэтому сбой exec не откатывает назначение, а только логируется.
    if let Err(e) = crate::ws_ops::replace_project(&executor, &git_url, &branch).await {
        tracing::warn!("failed to switch project on ws-{id}: {e}");
    }
    Ok(Json(ws))
}

/// Отпустить воркстейшн — только суперпользователь. Свободная станция (без
/// открытой сессии) сбрасывается в «не привязан к проекту» (project_id = 0),
/// файлы проекта очищаются; сам ws (под/сервис) не пересоздаётся.
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
/// открытой сессии на нём нет. Закрытие сессии освобождает воркстейшн.
async fn open_workstation_session(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<OpenWorkstationSessionRequest>,
) -> Result<Json<Chat>, StatusCode> {
    let user_id = current_user(&state, &headers).await?;
    let chat = match state
        .chat_store
        .open_workstation_session(id, payload.title.as_deref(), user_id)
        .await
    {
        Ok(chat) => chat,
        Err(SessionError::NotFound) => return Err(StatusCode::NOT_FOUND),
        Err(SessionError::WorkstationNotReady) => return Err(StatusCode::CONFLICT),
        Err(SessionError::WorkstationBusy) => return Err(StatusCode::CONFLICT),
        Err(SessionError::Forbidden) => return Err(StatusCode::FORBIDDEN),
        Err(SessionError::Db(_)) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    // Ручное восстановление после падения: если открытая сессия — продолжение
    // прерванной на упавшей станции, восстанавливаем файлы проекта из её ветки.
    // Сбой восстановления сессию не роняет — станция остаётся на чистом клоне,
    // ошибка уходит в лог.
    if let Ok(Some(prev)) = state.chat_store.continues_session_id(chat.id).await {
        if let Ok(Some(branch)) = state.chat_store.session_branch(prev).await {
            let executor = crate::workstation::executor_for_workstation(Some(id), &state.cluster);
            if let Err(e) = crate::ws_ops::restore_workspace(&executor, &branch).await {
                tracing::warn!("failed to restore session from ws into ws-{id}: {e}");
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LlmClient;
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
        let config = Config { sso: None };
        let llm_client = LlmClient::new("http://localhost:1/v1", None, "test-model");
        let cluster = Cluster {
            backend: crate::cluster::Backend::K8s,
            kubectl: "kubectl".into(),
            namespace: "default".into(),
            template: "/nonexistent.yaml".into(),
            image: "img".into(),
            wait_timeout_secs: 1,
        };
        let reactive = ReactiveRunner::new(
            llm_client.clone(),
            trace_store.clone(),
            chat_store.clone(),
            cluster.clone(),
        );
        let sso_verifier = if sso {
            Some(auth::JwtVerifier::from_jwks_json(auth::TEST_JWKS).unwrap())
        } else {
            None
        };
        let state = AppState {
            config,
            trace_store,
            chat_store,
            reactive,
            cluster,
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
            serde_json::json!({"project_id": 1, "name": "w1"}),
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
            .create_workstation(1, "ws-1", None)
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
    async fn switching_workstation_forbidden_for_participant() {
        let (state, file) = test_state(true).await;
        let headers = auth_headers("alice", &["participant"]);
        // Переключение воркстейшна на другой проект — процедура админа.
        let (status, _) = post_json(
            "/workstations/1/switch",
            &headers,
            state.clone(),
            serde_json::json!({"project_id": 2}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
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

    fn agent_json(name: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "description": format!("Правила {name}"),
            "tools": ["git", "make"],
            "max_iterations": 3,
            "model": null,
            "temperature": 0.7,
            "parent": null,
            "skills": [],
            "commands": []
        })
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
                    model: None,
                    temperature: 0.7,
                    parent: None,
                    skills: vec![],
                    commands: vec![],
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
                    model: None,
                    temperature: 0.5,
                    parent: None,
                    skills: vec![],
                    commands: vec![],
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
                    model: None,
                    temperature: 0.5,
                    parent: None,
                    skills: vec![],
                    commands: vec![],
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
        // Каталог: скилл и команда с версиями.
        let (status, body) = post_json(
            "/skills",
            &headers,
            state.clone(),
            serde_json::json!({
                "name": "review",
                "versions": [{"version": "1", "content": "Проверять диф"}]
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
                "versions": [{"version": "1", "content": "Выкатывать"}]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // Набор с деревом и данными агенту способностями.
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
                    "model": null,
                    "temperature": 0.7,
                    "parent": null,
                    "skills": [{"name": "review", "pinned_version": null}],
                    "commands": [{"name": "deploy", "pinned_version": "1"}]
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
        // с версиями, инструменты — всё в детали набора.
        assert!(body.contains("territory"));
        assert!(body.contains("folder"));
        assert!(body.contains("src"));
        assert!(body.contains("tools"));
        assert!(body.contains("git"));
        assert!(body.contains("skills"));
        assert!(body.contains("review"));
        assert!(body.contains("commands"));
        assert!(body.contains("pinned_version"));
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
                "versions": [{"version": "1", "content": "Проверять диф"}]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let skill_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_i64()
            .unwrap();
        // Переименование через PATCH сохраняет версии.
        let (status, _) = patch_json(
            &format!("/skills/{skill_id}"),
            &headers,
            state.clone(),
            serde_json::json!({ "name": "review2" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, body) = get("/skills", &headers, state.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("review2"));
        assert!(body.contains("Проверять диф"));
        // Несуществующая способность — 404.
        let (status, _) = patch_json(
            "/skills/999",
            &headers,
            state.clone(),
            serde_json::json!({ "name": "x" }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        // Удаление убирает из списка; повторное — 404.
        let (status, _) =
            delete_json(&format!("/skills/{skill_id}"), &headers, state.clone()).await;
        assert_eq!(status, StatusCode::OK);
        let (status, body) = get("/skills", &headers, state.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.contains("review2"));
        let (status, _) =
            delete_json(&format!("/skills/{skill_id}"), &headers, state.clone()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        // Команды — тот же каталог, те же правки.
        let (status, body) = post_json(
            "/commands",
            &headers,
            state.clone(),
            serde_json::json!({
                "name": "deploy",
                "versions": [{"version": "1", "content": "Выкатывать"}]
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
            .create_workstation(1, "ws-1", None)
            .await
            .unwrap();
        // Участник проходит проверку доступа к содержимому любого воркстейшна.
        // В тестовом окружении kubectl недоступен — исполнение падает с 500,
        // но это уже не 401/403: проверка видимости пройдена.
        let (status, _) = get("/workstations/1/tree", &headers, state.clone()).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
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
}
