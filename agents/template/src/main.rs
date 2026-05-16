use std::sync::Arc;
use tokio::sync::Mutex;

mod agent;
mod model;
mod tools;

use agent::{Agent, AgentConfig};
use model::{load_model, ModelConfig};
use tools::{FileTools, CodeTools};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
        )
        .init();

    println!("Agent starting...");

    // Load configuration from agent.md or environment
    let config = load_config()?;

    println!("Loaded config for agent: {}", config.name);

    // Initialize shared state
    let file_tools = Arc::new(Mutex::new(FileTools::new("/shared-files")));
    let code_tools = Arc::new(Mutex::new(CodeTools::new(file_tools.clone())));

    // Load LLM model
    let model_config = ModelConfig {
        name: std::env::var("MODEL_NAME").unwrap_or_else(|_| config.model.name.clone()),
        path: std::env::var("MODEL_PATH").unwrap_or_else(|_| config.model.path.clone()),
        temperature: std::env::var("MODEL_TEMPERATURE")
            .and_then(|s| s.parse().ok())
            .or(config.model.temperature),
        max_tokens: std::env::var("MODEL_MAX_TOKENS")
            .and_then(|s| s.parse().ok())
            .or(config.model.max_tokens),
    };

    println!("Loading model: {} at {}", model_config.name, model_config.path);
    
    // In production, this would load the actual model
    // For now, we'll use a placeholder
    let _model = load_model(&model_config).await?;

    // Create agent instance
    let agent = Agent::new(config, file_tools, code_tools);

    // Subscribe to tasks from NATS
    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://nats:4222".to_string());
    let nats_client = nats::connect(nats_url).await?;

    let subject = format!("agent.{}.task", config.name);
    let mut subscription = nats_client.subscribe(&subject).await?;

    println!("Agent {} subscribed to tasks: {}", config.name, subject);

    // Main loop - process incoming tasks
    loop {
        match subscription.next().await {
            Some(Ok((_, message))) => {
                if let Ok(task) = serde_json::from_str::<TaskRequest>(&message) {
                    println!("Received task: {:?}", task);

                    // Execute the task
                    match agent.run(task).await {
                        Ok(result) => {
                            println!("Task completed successfully");
                            
                            // Publish result
                            let result_subject = format!("agent.{}.result", config.name);
                            let _ = nats_client.publish(&result_subject, serde_json::to_string(&result).unwrap()).await;

                            // Update status
                            let status_subject = format!("agent.{}.status", config.name);
                            let _ = nats_client.publish(&status_subject, "completed").await;
                        }
                        Err(e) => {
                            eprintln!("Task failed: {}", e);
                            
                            // Publish error result
                            let result_subject = format!("agent.{}.result", config.name);
                            let error_result = serde_json::json!({
                                "success": false,
                                "error": Some(e.to_string()),
                                "result": None
                            });
                            let _ = nats_client.publish(&result_subject, error_result.to_string()).await;
                        }
                    }
                }
            }
            Some(Err(e)) => {
                eprintln!("Error receiving message: {}", e);
            }
            None => {
                println!("Subscription closed, shutting down...");
                break;
            }
        }
    }

    Ok(())
}

// ==================== Task Request Model ====================

#[derive(Debug, serde::Deserialize)]
struct TaskRequest {
    task: String,
    context: Option<String>,
    priority: Option<i32>,
    files: Option<Vec<String>>,
}

#[derive(Debug, serde::Serialize)]
struct TaskResult {
    success: bool,
    result: Option<String>,
    error: Option<String>,
    execution_time_ms: Option<u64>,
}

// ==================== Config Loading ====================

fn load_config() -> Result<AgentConfig, Box<dyn std::error::Error>> {
    // Try to load from agent.md file first
    if let Ok(config_path) = std::env::var("AGENT_CONFIG_PATH") {
        return load_config_from_file(&config_path);
    }

    // Fall back to environment variables
    let config = AgentConfig {
        name: std::env::var("AGENT_NAME").unwrap_or_else(|_| "unknown".to_string()),
        role: std::env::var("AGENT_ROLE").unwrap_or_else(|_| "unknown".to_string()),
        description: std::env::var("AGENT_DESCRIPTION").unwrap_or_default(),
        capabilities: parse_capabilities(&std::env::var("AGENT_CAPABILITIES")?),
        model: ModelConfig {
            name: std::env::var("MODEL_NAME").unwrap_or_else(|_| "codellama-13b".to_string()),
            path: std::env::var("MODEL_PATH").unwrap_or_else(|_| "/models/codellama-13b".to_string()),
            temperature: std::env::var("MODEL_TEMPERATURE")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.7),
            max_tokens: std::env::var("MODEL_MAX_TOKENS")
                .and_then(|s| s.parse().ok())
                .unwrap_or(4096),
        },
        nats_subject: format!("agent.{}.task", std::env::var("AGENT_NAME")?),
        status_topic: format!("agent.{}.status", std::env::var("AGENT_NAME")?),
        result_topic: format!("agent.{}.result", std::env::var("AGENT_NAME")?),
    };

    Ok(config)
}

fn load_config_from_file(path: &str) -> Result<AgentConfig, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    
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
        
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        if line.starts_with("role:") {
            config.role = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }
        else if line.starts_with("description:") {
            config.description = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }
        else if line.starts_with("- ") && !config.capabilities.iter().any(|c| c == &line.trim()[2..]) {
            let cap = line.trim()[2..].to_string();
            config.capabilities.push(cap);
        }
        else if line.starts_with("model:") {
            config.model.name = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }
        else if line.starts_with("model_path:") {
            config.model.path = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }
        else if line.starts_with("temperature:") {
            if let Ok(temp) = line.split(':').nth(1).and_then(|s| s.trim().parse::<f32>()) {
                config.model.temperature = temp;
            }
        }
        else if line.starts_with("max_tokens:") {
            if let Ok(tokens) = line.split(':').nth(1).and_then(|s| s.trim().parse::<u32>()) {
                config.model.max_tokens = tokens;
            }
        }
        else if line.starts_with("nats_subject:") {
            config.nats_subject = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }
        else if line.starts_with("status_topic:") {
            config.status_topic = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }
        else if line.starts_with("result_topic:") {
            config.result_topic = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }
    }

    let filename = std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    
    if let Some(name) = filename.strip_suffix(".md") {
        config.name = name.to_string();
    }

    Ok(config)
}

fn parse_capabilities(cap_str: &str) -> Vec<String> {
    cap_str.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
