use std::sync::Arc;
use tokio::sync::Mutex;

use super::{TaskRequest, TaskResult};

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub name: String,
    pub role: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub model: ModelConfig,
    pub nats_subject: String,
    pub status_topic: String,
    pub result_topic: String,
}

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub name: String,
    pub path: String,
    pub temperature: f32,
    pub max_tokens: u32,
}

pub struct Agent {
    config: AgentConfig,
    file_tools: Arc<Mutex<dyn FileTools>>,
    code_tools: Arc<Mutex<dyn CodeTools>>,
}

pub trait FileTools: Send + Sync {
    fn read_file(&self, path: &str) -> Result<String, String>;
    fn write_file(&self, path: &str, content: &str) -> Result<(), String>;
    fn list_directory(&self, path: &str) -> Result<Vec<String>, String>;
}

pub trait CodeTools: Send + Sync {
    fn execute_code(&self, code: &str) -> Result<String, String>;
    fn search_replace(&self, file_path: &str, old_text: &str, new_text: &str) -> Result<(), String>;
    fn analyze_code(&self, code: &str) -> Result<String, String>;
}

impl Agent {
    pub fn new(
        config: AgentConfig,
        file_tools: Arc<Mutex<dyn FileTools>>,
        code_tools: Arc<Mutex<dyn CodeTools>>,
    ) -> Self {
        Agent {
            config,
            file_tools,
            code_tools,
        }
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }

    pub fn role(&self) -> &str {
        &self.config.role
    }

    pub async fn run(&self, task: TaskRequest) -> Result<TaskResult, String> {
        println!("Agent {} received task: {}", self.config.name, task.task);

        // Check capabilities
        if !self.has_capability("code_generation") && !self.has_capability("code_analysis") {
            return Err(format!(
                "Agent {} does not have required capabilities for this task",
                self.config.name
            ));
        }

        // Process the task based on agent's role
        match self.config.role.as_str() {
            "code-generation" => {
                let result = self.generate_code(&task).await?;
                Ok(TaskResult {
                    success: true,
                    result: Some(result),
                    error: None,
                    execution_time_ms: Some(1500),
                })
            }
            "code-analysis" => {
                let result = self.analyze_code(&task).await?;
                Ok(TaskResult {
                    success: true,
                    result: Some(result),
                    error: None,
                    execution_time_ms: Some(1000),
                })
            }
            "code-review" => {
                let result = self.review_code(&task).await?;
                Ok(TaskResult {
                    success: true,
                    result: Some(result),
                    error: None,
                    execution_time_ms: Some(2000),
                })
            }
            "test-generation" => {
                let result = self.generate_tests(&task).await?;
                Ok(TaskResult {
                    success: true,
                    result: Some(result),
                    error: None,
                    execution_time_ms: Some(1800),
                })
            }
            "documentation" => {
                let result = self.generate_docs(&task).await?;
                Ok(TaskResult {
                    success: true,
                    result: Some(result),
                    error: None,
                    execution_time_ms: Some(1200),
                })
            }
            _ => Err(format!("Unknown agent role: {}", self.config.role)),
        }
    }

    async fn generate_code(&self, task: &TaskRequest) -> Result<String, String> {
        let prompt = format!(
            "Generate code for the following request:\n\nTask: {}\nContext: {:?}",
            task.task, task.context
        );

        // In production, this would call the LLM model
        // For now, return a placeholder response
        Ok(format!("Generated code for: {}", task.task))
    }

    async fn analyze_code(&self, task: &TaskRequest) -> Result<String, String> {
        let prompt = format!(
            "Analyze the following request:\n\nTask: {}\nContext: {:?}",
            task.task, task.context
        );

        // In production, this would call the LLM model
        Ok(format!("Analysis result for: {}", task.task))
    }

    async fn review_code(&self, task: &TaskRequest) -> Result<String, String> {
        let prompt = format!(
            "Review code for the following request:\n\nTask: {}\nContext: {:?}",
            task.task, task.context
        );

        // In production, this would call the LLM model
        Ok(format!("Code review result for: {}", task.task))
    }

    async fn generate_tests(&self, task: &TaskRequest) -> Result<String, String> {
        let prompt = format!(
            "Generate tests for the following request:\n\nTask: {}\nContext: {:?}",
            task.task, task.context
        );

        // In production, this would call the LLM model
        Ok(format!("Generated tests for: {}", task.task))
    }

    async fn generate_docs(&self, task: &TaskRequest) -> Result<String, String> {
        let prompt = format!(
            "Generate documentation for the following request:\n\nTask: {}\nContext: {:?}",
            task.task, task.context
        );

        // In production, this would call the LLM model
        Ok(format!("Generated documentation for: {}", task.task))
    }

    fn has_capability(&self, capability: &str) -> bool {
        self.config.capabilities.iter().any(|c| c == capability)
    }
}
