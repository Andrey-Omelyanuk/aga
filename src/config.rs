use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub roles: HashMap<String, RoleConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RoleConfig {
    pub prompt: String,
    pub target: Option<TargetConfig>,
    pub allowed_commands: Vec<String>,
    pub max_iterations: u32,
    pub llm: LlmConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TargetConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key_path: PathBuf,
    pub workdir: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LlmConfig {
    pub model: String,
    pub temperature: f32,
}

impl Config {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    pub fn get_role(&self, name: &str) -> Option<&RoleConfig> {
        self.roles.get(name)
    }
}
