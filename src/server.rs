use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio::sync::mpsc;
use futures_util::stream::{self, Stream};
use tower_http::services::ServeDir;

use crate::config::Config;
use crate::trace::TraceStore;
use crate::agent::Agent;
use crate::llm::LlmClient;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub trace_store: TraceStore,
    pub llm_client: LlmClient,
}

#[derive(Deserialize)]
pub struct TaskRequest {
    pub task: String,
    pub project_id: Option<i64>,
}

#[derive(Serialize)]
pub struct TaskResponse {
    pub status: String,
    pub task_id: String,
    pub result: String,
}

#[derive(Serialize)]
pub struct HumanRequestResponse {
    pub id: String,
    pub task_id: String,
    pub question: String,
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
        .route("/projects/:id/roles", get(get_project_roles).post(set_project_roles))
        .route("/roles", get(list_all_roles))
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
        .ok_or_else(|| StatusCode::NOT_FOUND)?
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
        Err(e) => Err(StatusCode::INTERNAL_SERVER_ERROR),
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
    let (tx, mut rx) = mpsc::channel::<String>(100);

    // Получаем текущие.pending запросы
    match state.trace_store.get_pending_human_requests().await {
        Ok(requests) => {
            for (id, task_id, question) in requests {
                let json = serde_json::json!({
                    "id": id,
                    "task_id": task_id,
                    "question": question,
                });
                let _ = tx.send(format!("data: {}\n\n", json)).await;
            }
        }
        Err(_) => {}
    }

    let stream = stream::unfold(rx, |mut rx| async move {
        if let Some(data) = rx.recv().await {
            Some((Ok(Event::default().data(data)), rx))
        } else {
            None
        }
    });

    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}

async fn answer_human_request(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<HumanAnswerRequest>,
) -> Result<StatusCode, StatusCode> {
    match state.trace_store.answer_human_request(&id, &payload.answer).await {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

// === API для управления проектами ===

async fn list_projects(State(state): State<AppState>) -> Result<Json<Vec<ProjectInfo>>, StatusCode> {
    match state.trace_store.get_all_projects().await {
        Ok(projects) => {
            let mut result = Vec::new();
            for project in projects {
                let active_roles = match state.trace_store.get_active_project_roles(project.id).await {
                    Ok(roles) => roles,
                    Err(_) => Vec::new(),
                };
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
    match state.trace_store.upsert_project(&payload.compose_path).await {
        Ok(project_id) => {
            let active_roles = match state.trace_store.get_active_project_roles(project_id).await {
                Ok(roles) => roles,
                Err(_) => Vec::new(),
            };
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
            let active_roles = match state.trace_store.get_active_project_roles(project.id).await {
                Ok(roles) => roles,
                Err(_) => Vec::new(),
            };
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
    // Получаем список всех доступных ролей из конфига
    let roles: Vec<String> = state.config.roles.keys().cloned().collect();
    Ok(Json(roles))
}
