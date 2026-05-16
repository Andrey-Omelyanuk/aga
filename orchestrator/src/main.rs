use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

mod agent_manager;
mod nats_client;
mod db;

use agent_manager::{AgentManager, AgentConfig};
use nats_client::NatsClient;
use db::{create_tables, get_agent_config_from_db};

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

    // Create tables if not exist
    create_tables(&db).await?;

    // Connect to NATS
    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://nats:4222".to_string());
    let nats_client = NatsClient::new(nats_url).await?;

    // Load agent configurations from files
    let config_dir = std::env::var("AGENT_CONFIG_DIR").unwrap_or_else(|_| "/config/agents".to_string());
    let mut agent_configs: HashMap<String, AgentConfig> = HashMap::new();

    if let Ok(entries) = std::fs::read_dir(&config_dir) {
        for entry in entries.flatten() {
            if entry.file_name().to_str().map_or(false, |s| s.ends_with(".md")) {
                if let Ok(config) = load_agent_config_from_file(entry.path()) {
                    agent_configs.insert(
                        config.name.clone(),
                        Arc::new(Mutex::new(config)),
                    );
                }
            }
        }
    }

    // Store in database
    for (name, config) in &agent_configs {
        let _ = db::save_agent_config(&db, name, config).await;
    }

    // Create agent manager
    let agent_manager = AgentManager::new(
        db.clone(),
        nats_client,
        agent_configs,
    );

    // Subscribe to task messages from NATS
    let mut task_subscriptions: HashMap<String, tokio::sync::mpsc::Sender<_>> = HashMap::new();
    
    for (agent_name, config) in &agent_configs {
        let config_clone = Arc::clone(config);
        let manager_clone = agent_manager.clone();
        
        let subject = format!("agent.{}.task", agent_name);
        let mut subscription = nats_client.subscribe(&subject).await?;

        tokio::spawn(async move {
            loop {
                match subscription.next().await {
                    Some(Ok((_, message))) => {
                        if let Ok(task) = serde_json::from_str::<TaskRequest>(&message) {
                            println!("Received task for agent {}: {:?}", agent_name, task);
                            
                            // Execute the task
                            let config_guard = config_clone.lock().await;
                            let result = manager_clone.execute_task(&task).await;
                            
                            drop(config_guard);

                            // Publish result
                            if let Ok(result) = &result {
                                let result_subject = format!("agent.{}.result", agent_name);
                                let _ = nats_client.publish(&result_subject, serde_json::to_string(result).unwrap()).await;
                            }
                        }
                    }
                    Some(Err(e)) => {
                        eprintln!("Error receiving message: {}", e);
                    }
                    None => break,
                }
            }
        });

        // Store subscription for later management
        task_subscriptions.insert(agent_name.clone(), manager_clone.task_sender());
    }

    println!("Orchestrator started. Listening for tasks...");

    // Keep running
    tokio::signal::ctrl_c().await?;
    println!("Shutting down orchestrator...");

    Ok(())
}

// ==================== Task Request Model ====================

#[derive(serde::Deserialize)]
struct TaskRequest {
    task: String,
    context: Option<String>,
    priority: Option<i32>,
    agent_name: Option<String>,
}

// ==================== Helper Functions ====================

fn load_agent_config_from_file(path: std::path::PathBuf) -> Result<AgentConfig, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(&path)?;
    
    // Parse agent.md file
    let mut config = AgentConfig {
        name: "unknown".to_string(),
        role: "unknown".to_string(),
        description: String::new(),
        capabilities: Vec::new(),
        model: ModelConfig {
            name: "codellama-13b".to_string(),
            path: "/models/codellama-13b".to_string(),
            temperature: 0.7,
            max_tokens: 4096,
        },
        nats_subject: String::new(),
        status_topic: String::new(),
        result_topic: String::new(),
    };

    for line in content.lines() {
        let line = line.trim();
        
        // Skip comments and empty lines
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        // Parse role
        if line.starts_with("role:") {
            config.role = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }
        
        // Parse description
        else if line.starts_with("description:") {
            config.description = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }

        // Parse capabilities (can span multiple lines)
        else if line.starts_with("- ") && !config.capabilities.iter().any(|c| c == &line.trim()[2..]) {
            let cap = line.trim()[2..].to_string();
            config.capabilities.push(cap);
        }

        // Parse model name
        else if line.starts_with("model:") {
            config.model.name = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }

        // Parse model path
        else if line.starts_with("model_path:") {
            config.model.path = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }

        // Parse temperature
        else if line.starts_with("temperature:") {
            if let Ok(temp) = line.split(':').nth(1).and_then(|s| s.trim().parse::<f32>()) {
                config.model.temperature = temp;
            }
        }

        // Parse max_tokens
        else if line.starts_with("max_tokens:") {
            if let Ok(tokens) = line.split(':').nth(1).and_then(|s| s.trim().parse::<u32>()) {
                config.model.max_tokens = tokens;
            }
        }

        // Parse NATS subject
        else if line.starts_with("nats_subject:") {
            config.nats_subject = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }

        // Parse status topic
        else if line.starts_with("status_topic:") {
            config.status_topic = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }

        // Parse result topic
        else if line.starts_with("result_topic:") {
            config.result_topic = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }
    }

    // Extract name from filename
    let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if let Some(name) = filename.strip_suffix(".md") {
        config.name = name.to_string();
    }

    Ok(config)
}

// ==================== Task Request for API ====================

#[derive(Debug, serde::Serialize)]
struct TaskResult {
    success: bool,
    result: Option<String>,
    error: Option<String>,
    execution_time_ms: Option<u64>,
    agent_name: String,
}
