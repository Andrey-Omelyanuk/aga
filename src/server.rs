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

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/tasks/:role", post(create_task))
        .route("/trace/:task_id", get(get_trace))
        .route("/human/pending", get(pending_human_requests))
        .route("/human/answer/:id", post(answer_human_request))
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
