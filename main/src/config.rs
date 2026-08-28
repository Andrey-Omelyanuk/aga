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
    /// Модель LLM. Пустая/отсутствующая — используется дефолт из LLM_MODEL.
    #[serde(default)]
    pub model: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_tmp(content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("aga_cfg_test_{}.yaml", uuid::Uuid::new_v4()));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn role_without_llm_model_loads_with_none() {
        let yaml = r#"
roles:
  r:
    prompt: p
    allowed_commands: ["echo"]
    max_iterations: 1
    llm:
      temperature: 0.5
"#;
        let path = write_tmp(yaml);
        let config = Config::load(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(&path).ok();

        let role = config.get_role("r").unwrap();
        assert!(role.llm.model.is_none());
        assert_eq!(role.llm.temperature, 0.5);
    }

    #[test]
    fn role_with_llm_model_loads_it() {
        let yaml = r#"
roles:
  r:
    prompt: p
    allowed_commands: ["echo"]
    max_iterations: 1
    llm:
      model: "deepseek-v4-flash"
      temperature: 0.3
"#;
        let path = write_tmp(yaml);
        let config = Config::load(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(&path).ok();

        let role = config.get_role("r").unwrap();
        assert_eq!(role.llm.model.as_deref(), Some("deepseek-v4-flash"));
        assert_eq!(role.llm.temperature, 0.3);
    }
}
