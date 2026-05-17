use reqwest::Client;
use serde::{Deserialize, Serialize};
use crate::config::LlmConfig;

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

pub struct LlmClient {
    client: Client,
    api_url: String,
    api_key: Option<String>,
}

impl LlmClient {
    pub fn new(api_url: &str, api_key: Option<String>) -> Self {
        Self {
            client: Client::new(),
            api_url: api_url.to_string(),
            api_key,
        }
    }

    pub async fn chat(
        &self,
        config: &LlmConfig,
        system_prompt: &str,
        user_message: &str,
        history: &[String],
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
        ];

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
            model: config.model.clone(),
            messages,
            temperature: config.temperature,
            max_tokens: Some(2048),
        };

        let mut req = self.client
            .post(&format!("{}/chat/completions", self.api_url))
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
