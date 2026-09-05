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

        // Адрес и ключ берутся из выбранного подключения, если оно задано;
        // иначе — дефолтные из env (url/ключ ядра).
        let api_url = config.api_url.as_deref().unwrap_or(&self.api_url);
        let api_key = config.api_key.as_ref().or(self.api_key.as_ref());
        let mut req = self
            .client
            .post(format!("{api_url}/chat/completions"))
            .json(&request)
            .header("Content-Type", "application/json");

        if let Some(key) = api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
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
            api_url: None,
            api_key: None,
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

    /// Мок LLM-эндпоинта: записывает путь и Authorization, отвечает фиксированно.
    type RecordedCalls = std::sync::Arc<tokio::sync::Mutex<Vec<(String, Option<String>)>>>;
    async fn mock_llm_server() -> (String, tokio::task::JoinHandle<()>, RecordedCalls) {
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let recorded: RecordedCalls = Arc::new(Mutex::new(Vec::new()));
        let recorded2 = recorded.clone();
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move |req: axum::extract::Request| {
                let recorded = recorded2.clone();
                async move {
                    let auth = req
                        .headers()
                        .get("authorization")
                        .and_then(|h| h.to_str().ok())
                        .map(|s| s.to_string());
                    let uri = req.uri().path().to_string();
                    recorded.lock().await.push((uri, auth));
                    axum::Json(serde_json::json!({
                        "choices": [{"message": {"role": "assistant", "content": "ok"}}]
                    }))
                }
            }),
        );
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}/v1"), handle, recorded)
    }

    #[tokio::test]
    async fn chosen_connection_sends_its_url_and_key() {
        let (api_url, server, recorded) = mock_llm_server().await;
        let client = LlmClient::new(
            "http://env-default/v1",
            Some("env-key".to_string()),
            "default-model",
        );
        let mut cfg = llm_config(None);
        cfg.api_url = Some(api_url);
        cfg.api_key = Some("conn-key".to_string());
        let resp = client.chat(&cfg, "sys", "hello", &[]).await.unwrap();
        assert_eq!(resp, "ok");
        let rec = recorded.lock().await;
        assert_eq!(rec.len(), 1);
        // Запрос ушёл на url и с ключом выбранного подключения, не env-дефолта.
        assert_eq!(rec[0].0, "/v1/chat/completions");
        assert_eq!(rec[0].1.as_deref(), Some("Bearer conn-key"));
        server.abort();
    }

    #[tokio::test]
    async fn agent_without_connection_uses_env_default_url_and_key() {
        let (api_url, server, recorded) = mock_llm_server().await;
        let client = LlmClient::new(&api_url, Some("env-key".to_string()), "default-model");
        let cfg = llm_config(None);
        let resp = client.chat(&cfg, "sys", "hello", &[]).await.unwrap();
        assert_eq!(resp, "ok");
        let rec = recorded.lock().await;
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].1.as_deref(), Some("Bearer env-key"));
        server.abort();
    }
}
