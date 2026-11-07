//! Проекты (git-URL) и привязка к ним набора агентов.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};

use super::{current_user, require_admin, AppError, AppState};

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

// === API для управления проектами ===

pub(super) async fn list_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ProjectInfo>>, AppError> {
    // Любой вошедший участник видит все проекты.
    current_user(&state, &headers).await?;
    let projects = state.trace_store.get_all_projects().await?;
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

pub(super) async fn create_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateProjectRequest>,
) -> Result<Json<ProjectInfo>, AppError> {
    current_user(&state, &headers).await?;
    // git_url уходит в git clone на воркстейшне — мусор отсекаем на входе.
    let git_url = payload.git_url.trim().to_string();
    crate::ws_ops::validate_git_url(&git_url).map_err(|_| StatusCode::BAD_REQUEST)?;
    let project_id = state.trace_store.upsert_project(&git_url).await?;
    let agent_set = state
        .trace_store
        .get_project_agent_set(project_id)
        .await
        .unwrap_or(None);
    Ok(Json(ProjectInfo {
        id: project_id,
        git_url,
        agent_set,
    }))
}

pub(super) async fn get_project(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ProjectInfo>, AppError> {
    current_user(&state, &headers).await?;
    let project = state
        .trace_store
        .get_project(id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
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

pub(super) async fn delete_project(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    require_admin(&state, &headers).await?;
    if state.trace_store.delete_project(id).await? {
        Ok(StatusCode::OK)
    } else {
        Err(StatusCode::NOT_FOUND.into())
    }
}

// === Набор агентов проекта ===

pub(super) async fn attach_agent_set(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AttachAgentSetRequest>,
) -> Result<StatusCode, AppError> {
    current_user(&state, &headers).await?;
    if state
        .trace_store
        .get_project(id)
        .await
        .unwrap_or(None)
        .is_none()
    {
        return Err(StatusCode::NOT_FOUND.into());
    }
    state
        .trace_store
        .attach_agent_set(id, payload.agent_set_id)
        .await?;
    Ok(StatusCode::OK)
}

pub(super) async fn get_project_agent_set(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::trace::AgentSet>, AppError> {
    current_user(&state, &headers).await?;
    let agent_set = state
        .trace_store
        .get_project_agent_set(id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(agent_set))
}
