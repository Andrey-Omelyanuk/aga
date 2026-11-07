//! Каталог MCP-серверов: http (Streamable HTTP по url) или stdio (команда в
//! воркстейшне агента); проверка инструментов http-сервера из UI.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Serialize;

use super::{current_user, merge_secret, new_secret, require_admin, AppError, AppState};

/// MCP-сервер наружу: ключ Bearer не отдаётся (только `has_api_key`).
#[derive(Serialize)]
pub struct McpServerView {
    pub id: i64,
    pub name: String,
    pub transport: String,
    pub url: String,
    pub command: String,
    pub has_api_key: bool,
}

impl From<crate::trace::McpServer> for McpServerView {
    fn from(s: crate::trace::McpServer) -> Self {
        Self {
            id: s.id,
            name: s.name,
            transport: s.transport,
            url: s.url,
            command: s.command,
            has_api_key: s.api_key.as_deref().is_some_and(|k| !k.is_empty()),
        }
    }
}

// === API каталога MCP-серверов ===

pub(super) async fn list_mcp_servers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<McpServerView>>, AppError> {
    current_user(&state, &headers).await?;
    let list = state.trace_store.list_mcp_servers().await?;
    Ok(Json(list.into_iter().map(McpServerView::from).collect()))
}

pub(super) async fn get_mcp_server(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<McpServerView>, AppError> {
    current_user(&state, &headers).await?;
    let server = state
        .trace_store
        .get_mcp_server(id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(server.into()))
}

/// Проверка спека: заполнение (400) и уникальность имени (409).
async fn check_mcp_spec(
    state: &AppState,
    spec: &crate::trace::McpServerSpec,
    exclude_id: i64,
) -> Result<(), AppError> {
    spec.validate().map_err(|_| StatusCode::BAD_REQUEST)?;
    if state
        .trace_store
        .mcp_server_name_taken(spec.name.trim(), exclude_id)
        .await?
    {
        return Err(StatusCode::CONFLICT.into());
    }
    Ok(())
}

pub(super) async fn create_mcp_server(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut spec): Json<crate::trace::McpServerSpec>,
) -> Result<Json<McpServerView>, AppError> {
    require_admin(&state, &headers).await?;
    check_mcp_spec(&state, &spec, 0).await?;
    spec.api_key = new_secret(spec.api_key.take());
    let id = state.trace_store.create_mcp_server(&spec).await?;
    let server = state
        .trace_store
        .get_mcp_server(id)
        .await?
        .ok_or_else(|| AppError::internal(format!("created mcp server {id} not found")))?;
    Ok(Json(server.into()))
}

pub(super) async fn update_mcp_server(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut spec): Json<crate::trace::McpServerSpec>,
) -> Result<Json<McpServerView>, AppError> {
    require_admin(&state, &headers).await?;
    check_mcp_spec(&state, &spec, id).await?;
    let old = state
        .trace_store
        .get_mcp_server(id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    spec.api_key = merge_secret(spec.api_key.take(), old.api_key);
    if !state.trace_store.update_mcp_server(id, &spec).await? {
        return Err(StatusCode::NOT_FOUND.into());
    }
    let server = state
        .trace_store
        .get_mcp_server(id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(server.into()))
}

pub(super) async fn delete_mcp_server(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    require_admin(&state, &headers).await?;
    if state.trace_store.delete_mcp_server(id).await? {
        Ok(StatusCode::OK)
    } else {
        Err(StatusCode::NOT_FOUND.into())
    }
}

/// Проверка http-сервера: подключиться и вернуть его инструменты. Сбой
/// подключения — 502 с текстом ошибки. stdio-сервер запускается только в
/// воркстейшне агента — отсюда не проверяется (400). Ошибки ручка отдаёт
/// текстом (`(StatusCode, String)`), сбой БД — пустым текстом с 500 и
/// причиной в логе (`AppError::into_text`).
pub(super) async fn mcp_server_tools(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::mcp::McpTool>>, (StatusCode, String)> {
    current_user(&state, &headers)
        .await
        .map_err(|code| (code, String::new()))?;
    let server = state
        .trace_store
        .get_mcp_server(id)
        .await
        .map_err(|e| AppError::from(e).into_text())?
        .ok_or((StatusCode::NOT_FOUND, String::new()))?;
    if server.transport != "http" {
        return Err((
            StatusCode::BAD_REQUEST,
            "stdio-сервер запускается в воркстейшне агента и отсюда не проверяется".into(),
        ));
    }
    // У http-сервера процесса нет — запуск не понадобится.
    let launch = |c: &str| {
        crate::agent::exec_command(
            &crate::agent::Executor::Sh,
            c,
            true,
            crate::agent::RunAs::Station,
        )
    };
    let mut client = crate::mcp::McpClient::connect(&server, launch)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
    client
        .list_tools()
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}
