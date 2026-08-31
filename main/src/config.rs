use serde::{Deserialize, Serialize};

/// Конфигурация ядра. Ролей больше нет — их заменили AgentSet-ы, которые живут
/// в БД и настраиваются через API. Здесь остался только SSO.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
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
    /// Адрес end-session эндпоинта Keycloak (для `/auth/logout`).
    #[serde(default)]
    pub end_session_url: Option<String>,
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
    /// Инструменты — список исполняемого в консоли воркстейшна, версий у них нет.
    pub tools: Vec<String>,
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
    fn config_loads_without_roles_and_ignores_unknown_keys() {
        // Роли из прежнего roles.yaml сервер больше не читает: конфиг загружает
        // только SSO, а посторонние ключи (roles:) игнорируются serde.
        let yaml = r#"
roles:
  app-deployer:
    prompt: p
    allowed_commands: ["echo"]
    max_iterations: 1
    llm:
      temperature: 0.7
sso:
  enabled: false
"#;
        let path = write_tmp(yaml);
        let config = Config::load(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(&path).ok();
        assert!(config.sso.is_some());
    }
}
