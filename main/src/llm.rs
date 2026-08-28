use crate::config::LlmConfig;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct LlmRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct LlmResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChatMessage,
}

#[derive(Debug, Clone)]
pub struct LlmClient {
    client: Client,
    api_url: String,
    api_key: Option<String>,
    default_model: String,
}

impl LlmClient {
    pub fn new(api_url: &str, api_key: Option<String>, default_model: &str) -> Self {
        Self {
            client: Client::new(),
            api_url: api_url.to_string(),
            api_key,
            default_model: default_model.to_string(),
        }
    }

    /// Модель роли, если задана и не пустая; иначе дефолтная из LLM_MODEL.
    fn resolve_model(&self, config: &LlmConfig) -> String {
        config
            .model
            .clone()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| self.default_model.clone())
    }

    pub async fn chat(
        &self,
        config: &LlmConfig,
        system_prompt: &str,
        user_message: &str,
        history: &[String],
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut messages = vec![ChatMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        }];

        // Добавляем историю диалога
        for (i, msg) in history.iter().enumerate() {
            let role = if i % 2 == 0 { "user" } else { "assistant" };
            messages.push(ChatMessage {
                role: role.to_string(),
                content: msg.clone(),
            });
        }

        // Добавляем текущее сообщение
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: user_message.to_string(),
        });

        let request = LlmRequest {
            model: self.resolve_model(config),
            messages,
            temperature: config.temperature,
            max_tokens: Some(2048),
        };

        let mut req = self
            .client
            .post(format!("{}/chat/completions", self.api_url))
            .json(&request)
            .header("Content-Type", "application/json");

        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let response = req.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("LLM API error {}: {}", status, body).into());
        }

        let llm_response: LlmResponse = response.json().await?;

        llm_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| "No response from LLM".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn llm_config(model: Option<&str>) -> LlmConfig {
        LlmConfig {
            model: model.map(|m| m.to_string()),
            temperature: 0.7,
        }
    }

    #[test]
    fn default_model_used_when_role_model_unset() {
        let client = LlmClient::new("http://x/v1", None, "default-model");
        assert_eq!(client.resolve_model(&llm_config(None)), "default-model");
    }

    #[test]
    fn default_model_used_when_role_model_empty() {
        let client = LlmClient::new("http://x/v1", None, "default-model");
        assert_eq!(client.resolve_model(&llm_config(Some(""))), "default-model");
        assert_eq!(
            client.resolve_model(&llm_config(Some("  "))),
            "default-model"
        );
    }

    #[test]
    fn role_model_overrides_default() {
        let client = LlmClient::new("http://x/v1", None, "default-model");
        assert_eq!(
            client.resolve_model(&llm_config(Some("role-model"))),
            "role-model"
        );
    }
}
