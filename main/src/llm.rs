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
}

impl Default for LlmClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    pub async fn chat(
        &self,
        config: &LlmConfig,
        system_prompt: &str,
        user_message: &str,
        history: &[String],
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Адрес обязателен: дефолтной LLM из env больше нет — url, ключ и
        // модель живут в подключении (выбранном агентом или дефолтном).
        let api_url = config.api_url.as_deref().ok_or_else(|| {
            "LLM не настроена: у агента нет подключения и дефолтная LLM не выбрана".to_string()
        })?;

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
            model: config.model.clone().unwrap_or_default(),
            messages,
            temperature: config.temperature,
            max_tokens: Some(2048),
        };

        let mut req = self
            .client
            .post(format!("{api_url}/chat/completions"))
            .json(&request)
            .header("Content-Type", "application/json");

        if let Some(key) = config.api_key.as_ref() {
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

    fn llm_config(model: &str, api_url: Option<&str>) -> LlmConfig {
        LlmConfig {
            model: Some(model.to_string()),
            temperature: 0.7,
            api_url: api_url.map(|u| u.to_string()),
            api_key: None,
        }
    }

    /// Мок LLM-эндпоинта: записывает путь, Authorization и модель, отвечает
    /// фиксированно.
    type RecordedCalls = std::sync::Arc<tokio::sync::Mutex<Vec<(String, Option<String>, String)>>>;
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
                    let body: serde_json::Value = axum::body::to_bytes(req.into_body(), usize::MAX)
                        .await
                        .ok()
                        .and_then(|b| serde_json::from_slice(&b).ok())
                        .unwrap_or_default();
                    let model = body["model"].as_str().unwrap_or_default().to_string();
                    recorded.lock().await.push((uri, auth, model));
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
    async fn chat_uses_config_url_key_and_model() {
        let (api_url, server, recorded) = mock_llm_server().await;
        let client = LlmClient::new();
        let mut cfg = llm_config("qwen3:0.6b", Some(&api_url));
        cfg.api_key = Some("conn-key".to_string());
        let resp = client.chat(&cfg, "sys", "hello", &[]).await.unwrap();
        assert_eq!(resp, "ok");
        let rec = recorded.lock().await;
        assert_eq!(rec.len(), 1);
        // Запрос ушёл на url подключения, с его ключом и его моделью.
        assert_eq!(rec[0].0, "/v1/chat/completions");
        assert_eq!(rec[0].1.as_deref(), Some("Bearer conn-key"));
        assert_eq!(rec[0].2, "qwen3:0.6b");
        server.abort();
    }

    #[tokio::test]
    async fn chat_without_configured_llm_fails() {
        let client = LlmClient::new();
        let cfg = llm_config("qwen3:0.6b", None);
        let err = client.chat(&cfg, "sys", "hello", &[]).await.unwrap_err();
        assert!(err.to_string().contains("LLM не настроена"));
    }
}
