use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

// Shared state for agent execution
pub struct AppState {
    pub nats_client: Option<nats::Client>,
    pub db: PgPool,
    pub shared_files: Arc<Mutex<HashMap<String, String>>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to database
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| format!("postgres://{}:{}@{}:{}/{}", 
            std::env::var("DB_USER").unwrap_or_else(|_| "agent_user".to_string()),
            std::env::var("DB_PASSWORD").unwrap_or_else(|_| "agent_pass".to_string()),
            std::env::var("DB_HOST").unwrap_or_else(|_| "postgres".to_string()),
            std::env::var("DB_PORT").unwrap_or_else(|_| "5432".to_string()),
        ));

    let db = PgPool::connect(&db_url).await?;

    // Connect to NATS
    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://nats:4222".to_string());
    let nats_client = nats::connect(nats_url).await?;

    // Initialize shared files storage
    let shared_files = Arc::new(Mutex::new(HashMap::new()));

    // Create state
    let state = AppState {
        nats_client: Some(nats_client),
        db,
        shared_files,
    };

    // Setup routes
    let app = Router::<State<AppState>>::new()
        .route("/agents", get(list_agents))
        .route("/agents/:name/status", get(agent_status))
        .route("/agents/:name/execute", post(execute_agent))
        .route("/shared-files", get(list_shared_files))
        .route("/shared-files/upload", post(upload_file))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ==================== Agent Routes ====================

#[derive(Debug, Serialize)]
struct Agent {
    name: String,
    role: String,
    status: String,
    capabilities: Vec<String>,
}

async fn list_agents(State(state): State<AppState>) -> Json<Vec<Agent>> {
    let agents = get_registered_agents(&state.db).await;
    Json(agents)
}

#[derive(Debug, Deserialize)]
struct TaskRequest {
    task: String,
    context: Option<String>,
    priority: Option<i32>,
}

#[derive(Debug, Serialize)]
struct AgentResponse {
    success: bool,
    result: Option<String>,
    error: Option<String>,
    execution_time_ms: Option<u64>,
}

async fn agent_status(
    State(state): State<AppState>,
    path: axum::extract::Path<String>,
) -> Json<Agent> {
    let agent_name = path.as_str();
    
    // Check NATS for agent status
    if let Some(ref client) = state.nats_client {
        let subject = format!("agent.{}.status", agent_name);
        
        // Try to get latest status message
        match client.subscribe(&subject).await {
            Ok(mut subscription) => {
                // In production, we'd track status in DB
                Json(Agent {
                    name: agent_name.to_string(),
                    role: format!("{} Agent", agent_name),
                    status: "running".to_string(),
                    capabilities: vec!["code_generation".to_string()],
                })
            }
            Err(_) => Json(Agent {
                name: agent_name.to_string(),
                role: format!("{} Agent", agent_name),
                status: "unknown".to_string(),
                capabilities: Vec::new(),
            }),
        }
    } else {
        Json(Agent {
            name: agent_name.to_string(),
            role: format!("{} Agent", agent_name),
            status: "unknown".to_string(),
            capabilities: Vec::new(),
        })
    }
}

async fn execute_agent(
    State(state): State<AppState>,
    path: axum::extract::Path<String>,
    Json(req): Json<TaskRequest>,
) -> impl IntoResponse {
    let agent_name = path.as_str();
    
    // Publish task to NATS topic for the specific agent
    if let Some(ref client) = state.nats_client {
        let subject = format!("agent.{}.task", agent_name);
        
        let message = serde_json::json!({
            "task": req.task,
            "context": req.context,
            "priority": req.priority,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        match client.publish(&subject, message.to_string()).await {
            Ok(_) => (StatusCode::ACCEPTED, Json(AgentResponse {
                success: true,
                result: None,
                error: None,
                execution_time_ms: None,
            })).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(AgentResponse {
                success: false,
                result: None,
                error: Some(format!("Failed to publish task: {}", e)),
                execution_time_ms: None,
            })).into_response(),
        }
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(AgentResponse {
            success: false,
            result: None,
            error: Some("NATS client not available".to_string()),
            execution_time_ms: None,
        })).into_response()
    }
}

// ==================== Shared Files Routes ====================

async fn list_shared_files(State(state): State<AppState>) -> Json<Vec<String>> {
    let files = state.shared_files.lock().await.clone();
    Json(files.into_keys().collect())
}

async fn upload_file(
    State(state): State<AppState>,
    path: axum::extract::Path<String>,
    mut body: axum::body::Bytes,
) -> impl IntoResponse {
    let file_name = path.as_str();
    
    // Store file content in memory (in production, use actual file system)
    let content = String::from_utf8_lossy(&body).to_string();
    
    state.shared_files.lock().await.insert(file_name.to_string(), content);
    
    (StatusCode::CREATED, Json({
        let files = state.shared_files.lock().await.clone();
        serde_json::json!({
            "filename": file_name,
            "size": body.len(),
            "uploaded_at": chrono::Utc::now().to_rfc3339()
        })
    })).into_response()
}

// ==================== Helper Functions ====================

async fn get_registered_agents(db: &PgPool) -> Vec<Agent> {
    // In production, query from database
    vec![
        Agent {
            name: "code-analysis".to_string(),
            role: "Code Analysis Agent".to_string(),
            status: "running".to_string(),
            capabilities: vec!["code_analysis".to_string(), "understanding".to_string()],
        },
        Agent {
            name: "code-generation".to_string(),
            role: "Code Generation Agent".to_string(),
            status: "running".to_string(),
            capabilities: vec!["code_generation".to_string(), "refactoring".to_string()],
        },
        Agent {
            name: "code-review".to_string(),
            role: "Code Review Agent".to_string(),
            status: "running".to_string(),
            capabilities: vec!["code_review".to_string(), "static_analysis".to_string()],
        },
        Agent {
            name: "test-generation".to_string(),
            role: "Test Generation Agent".to_string(),
            status: "running".to_string(),
            capabilities: vec!["test_generation".to_string(), "unit_testing".to_string()],
        },
        Agent {
            name: "documentation".to_string(),
            role: "Documentation Agent".to_string(),
            status: "running".to_string(),
            capabilities: vec!["documentation".to_string(), "doc_generation".to_string()],
        },
    ]
}
