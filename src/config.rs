use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub roles: HashMap<String, RoleConfig>,
    #[serde(default)]
    pub sso: Option<SsoConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SsoConfig {
    /// Если включено — API принимает Bearer-токены и сопоставляет `sub` с chat_user.
    #[serde(default)]
    pub enabled: bool,
    /// Проверка подписи JWT против JWKS (Keycloak `/protocol/openid-connect/certs`).
    #[serde(default)]
    pub jwks_url: Option<String>,
    /// Адрес authorize-эндпоинта Keycloak (для `/auth/login`).
    #[serde(default)]
    pub authorize_url: Option<String>,
    /// Адрес token-эндпоинта Keycloak (для `/auth/callback`).
    #[serde(default)]
    pub token_url: Option<String>,
    /// Идентификатор клиента aga в Keycloak.
    #[serde(default)]
    pub client_id: Option<String>,
    /// Секрет клиента aga в Keycloak.
    #[serde(default)]
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RoleConfig {
    pub prompt: String,
    pub allowed_commands: Vec<String>,
    pub max_iterations: u32,
    pub llm: LlmConfig,
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
