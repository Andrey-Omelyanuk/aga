//! Наборы агентов (AgentSet): состав, дерево агентов, их инструменты и скиллы.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use super::{current_user, AppError, AppState};

#[derive(Deserialize)]
pub struct CreateAgentSetRequest {
    pub name: String,
    pub agents: Vec<crate::trace::AgentSpec>,
}

/// Имена агентов набора — пути папок проекта (`scope::validate_agent_name`).
fn check_agent_names(agents: &[crate::trace::AgentSpec]) -> Result<(), StatusCode> {
    for agent in agents {
        crate::scope::validate_agent_name(&agent.name).map_err(|_| StatusCode::BAD_REQUEST)?;
    }
    Ok(())
}

// === API для управления наборами агентов ===

pub(super) async fn list_agent_sets(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::trace::AgentSet>>, AppError> {
    current_user(&state, &headers).await?;
    Ok(Json(state.trace_store.list_agent_sets().await?))
}

pub(super) async fn create_agent_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateAgentSetRequest>,
) -> Result<Json<crate::trace::AgentSet>, AppError> {
    current_user(&state, &headers).await?;
    check_agent_names(&payload.agents)?;
    let set_id = state
        .trace_store
        .create_agent_set(&payload.name, &payload.agents)
        .await?;
    let agent_set = state
        .trace_store
        .get_agent_set(set_id)
        .await?
        .ok_or_else(|| AppError::internal(format!("created agent set {set_id} not found")))?;
    Ok(Json(agent_set))
}

pub(super) async fn get_agent_set(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::trace::AgentSet>, AppError> {
    current_user(&state, &headers).await?;
    let agent_set = state
        .trace_store
        .get_agent_set(id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(agent_set))
}

pub(super) async fn delete_agent_set(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    current_user(&state, &headers).await?;
    if state.trace_store.delete_agent_set(id).await? {
        Ok(StatusCode::OK)
    } else {
        Err(StatusCode::NOT_FOUND.into())
    }
}

/// Полностью заменить состав набора (имя, агенты с их территорией,
/// инструментами и данными скиллами). Возвращает обновлённый набор.
pub(super) async fn update_agent_set(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateAgentSetRequest>,
) -> Result<Json<crate::trace::AgentSet>, AppError> {
    current_user(&state, &headers).await?;
    check_agent_names(&payload.agents)?;
    if !state
        .trace_store
        .update_agent_set(id, &payload.name, &payload.agents)
        .await?
    {
        return Err(StatusCode::NOT_FOUND.into());
    }
    let agent_set = state
        .trace_store
        .get_agent_set(id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(agent_set))
}
