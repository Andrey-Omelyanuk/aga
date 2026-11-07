//! HTTP API ядра (Axum). Хендлеры разбиты по доменам:
//! - `projects` — проекты и привязка набора агентов;
//! - `agent_sets` — наборы агентов;
//! - `llm` — подключения к LLM и дефолтная LLM;
//! - `mcp` — каталог MCP-серверов и проверка их инструментов;
//! - `capabilities` — каталог способностей: скиллы, сокращения, история;
//! - `sso` — вход веб-клиента через Keycloak (`/auth/*`);
//! - `chat` — пользователи, чаты, сообщения, нити, шаринг, артефакты;
//! - `workstations` — станции, сессии, содержимое и изменения проекта,
//!   SSH-ключ, трасса;
//! - `error` — `AppError`: ошибка хендлеров (причина 500 — в лог, не клиенту).
//!
//! Здесь — состояние (`AppState`), вся таблица маршрутов (`create_router`),
//! гейт токена и общие помощники хендлеров.

use axum::{
    extract::State,
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderMap, HeaderValue, Method, StatusCode,
    },
    response::Response,
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::auth;
use crate::centrifuge::CentrifugeClient;
use crate::chat::ChatStore;
use crate::cluster::Cluster;
use crate::config::Config;
use crate::trace::TraceStore;

mod agent_sets;
mod capabilities;
mod chat;
mod error;
mod llm;
mod mcp;
mod projects;
mod sso;
mod workstations;

#[cfg(test)]
mod tests;

use agent_sets::{
    create_agent_set, delete_agent_set, get_agent_set, list_agent_sets, update_agent_set,
};
use capabilities::{
    capability_history, create_shortcut, create_skill, delete_shortcut, delete_skill, get_shortcut,
    get_skill, list_shortcuts, list_skills, update_shortcut, update_skill,
};
use chat::{
    add_participant, close_chat, connection_jwt, create_chat, get_chat, get_user,
    list_chat_messages, list_chats, list_users, me, message_artifacts, post_message_to_parent,
    remove_participant, send_message, share_message, start_message_thread,
};
use error::AppError;
use llm::{
    create_llm_connection, delete_llm_connection, get_llm_connection, list_llm_connections,
    set_llm_default, update_llm_connection,
};
use mcp::{
    create_mcp_server, delete_mcp_server, get_mcp_server, list_mcp_servers, mcp_server_tools,
    update_mcp_server,
};
use projects::{
    attach_agent_set, create_project, delete_project, get_project, get_project_agent_set,
    list_projects,
};
use sso::{auth_callback, auth_login, auth_logout, auth_refresh};
use workstations::{
    create_workstation, delete_workstation, get_ssh_key, get_trace, get_workstation_session,
    list_workstations, mark_workstation_down, open_workstation_session, release_workstation,
    workstation_changes, workstation_file, workstation_tree,
};

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
async fn current_actor(state: &AppState, headers: &HeaderMap) -> Result<(i64, String), AppError> {
    let id = current_user(state, headers).await?;
    let name = state
        .chat_store
        .get_user(id)
        .await?
        .map(|u| u.name)
        .unwrap_or_else(|| format!("#{id}"));
    Ok((id, name))
}

/// Текущий пользователь-админ: realm-роль `admin` (суперпользователь чата);
/// без SSO — аноним-суперпользователь. Не админ — 403. Настройки платформы
/// (подключения к LLM, каталог MCP, удаление проектов) меняет только админ:
/// подменой url LLM участник мог бы уводить контекст всех агентов.
async fn require_admin(state: &AppState, headers: &HeaderMap) -> Result<i64, AppError> {
    let id = current_user(state, headers).await?;
    let is_admin = state.chat_store.is_super_user(id).await?;
    if is_admin {
        Ok(id)
    } else {
        Err(StatusCode::FORBIDDEN.into())
    }
}

/// Ключ из запроса правки: не задан (`null`/нет поля) — оставить прежний,
/// пустая строка — удалить, иначе — новый. Ключи наружу не отдаются, поэтому
/// форма правки их не знает и не должна затирать.
fn merge_secret(new: Option<String>, old: Option<String>) -> Option<String> {
    match new {
        None => old,
        Some(k) if k.trim().is_empty() => None,
        Some(k) => Some(k),
    }
}

/// Ключ из запроса создания: пустая строка — ключа нет.
fn new_secret(key: Option<String>) -> Option<String> {
    key.filter(|k| !k.trim().is_empty())
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
    // Публичные роуты: вход через SSO и проба живости. Всё остальное — за
    // гейтом `require_token` (route_layer: неизвестный путь — 404, не 401).
    let public = Router::new()
        .route("/healthz", get(healthz))
        // === SSO (Keycloak): вход веб-клиента ===
        .route("/auth/login", get(auth_login))
        .route("/auth/callback", get(auth_callback))
        .route("/auth/refresh", post(auth_refresh))
        .route("/auth/logout", get(auth_logout));
    let protected = Router::new()
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
        // === Каталог MCP-серверов ===
        // http (Streamable HTTP по url) или stdio (команда в воркстейшне
        // агента). Агент набора получает инструменты серверов по имени.
        .route(
            "/mcp-servers",
            get(list_mcp_servers).post(create_mcp_server),
        )
        .route(
            "/mcp-servers/:id",
            get(get_mcp_server)
                .patch(update_mcp_server)
                .delete(delete_mcp_server),
        )
        .route("/mcp-servers/:id/tools", get(mcp_server_tools))
        // === Каталог способностей (скиллы и сокращения) ===
        // У записи одно текущее содержимое и история изменений (кто, когда и
        // что сделал). ?deleted=1 — список «Удалённые» (история переживает
        // удаление). Фиксации версий нет: агент всегда берёт последнее.
        .route("/skills", get(list_skills).post(create_skill))
        .route(
            "/skills/:id",
            get(get_skill).patch(update_skill).delete(delete_skill),
        )
        .route("/skills/:id/history", get(capability_history))
        // === Сокращения (shortcuts) ===
        // Глобальный перечень: при отправке сообщения слово `/имя` добавляет
        // привязанный текст в скрытую часть сообщения. Живёт в том же каталоге,
        // что скиллы и сокращения (kind='shortcut'), с той же историей изменений;
        // агентам не даётся. Имя — без пробелов, уникально.
        .route("/shortcuts", get(list_shortcuts).post(create_shortcut))
        .route(
            "/shortcuts/:id",
            get(get_shortcut)
                .patch(update_shortcut)
                .delete(delete_shortcut),
        )
        .route("/shortcuts/:id/history", get(capability_history))
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
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_token,
        ));
    public.merge(protected).layer(cors).with_state(state)
}

/// Гейт аутентификации всех непубличных роутов: при включённом SSO запрос
/// без действительного токена получает 401 ещё до хендлера. Хендлеры всё равно
/// берут пользователя через `current_user` (и роли — через `require_admin`);
/// гейт лишь не даёт новому роуту остаться открытым по забывчивости.
async fn require_token(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<Response, StatusCode> {
    {
        let verifier = state.sso_verifier.read().await;
        auth::check_token(request.headers(), verifier.as_ref())?;
    }
    Ok(next.run(request).await)
}

/// Проба живости HTTP-сервера (k8s readiness/liveness): без авторизации и БД.
async fn healthz() -> &'static str {
    "ok"
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
