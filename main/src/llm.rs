use crate::config::LlmConfig;
use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Serialize)]
struct LlmRequest<'a> {
    model: String,
    messages: &'a [Message],
    temperature: f32,
    max_tokens: Option<i32>,
    /// Описания инструментов (нативный function calling); пусто — поле не
    /// отправляется (текстовый режим).
    #[serde(skip_serializing_if = "<[serde_json::Value]>::is_empty")]
    tools: &'a [serde_json::Value],
}

/// Сообщение диалога в формате OpenAI-compatible API: system/user/assistant
/// (у ассистента — вызовы инструментов) и tool (результат вызова).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    /// Текст сообщения. В ответе ассистента с вызовами инструментов бывает null.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Ответ на вызов инструмента (role = tool): id вызова.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    fn with(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
            ..Self::default()
        }
    }

    pub fn system(content: &str) -> Self {
        Self::with("system", content)
    }

    pub fn user(content: &str) -> Self {
        Self::with("user", content)
    }

    pub fn assistant(content: &str) -> Self {
        Self::with("assistant", content)
    }

    /// Результат вызова инструмента `call_id`.
    pub fn tool(call_id: &str, content: &str) -> Self {
        Self {
            tool_call_id: Some(call_id.to_string()),
            ..Self::with("tool", content)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type", default = "function_kind")]
    pub kind: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// Аргументы JSON-строкой (как в OpenAI). Часть серверов (Ollama) отдаёт
    /// объект — он приводится к строке.
    #[serde(default, deserialize_with = "arguments_as_string")]
    pub arguments: String,
}

fn function_kind() -> String {
    "function".to_string()
}

fn null_as_empty<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

fn arguments_as_string<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(s) => s,
        serde_json::Value::Null => "{}".to_string(),
        other => other.to_string(),
    })
}

#[derive(Debug, Deserialize)]
struct LlmResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
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

    /// Один запрос chat/completions: диалог `messages` и описания инструментов
    /// `tools` (пусто — без function calling). Возвращает сообщение ассистента:
    /// текст и, в нативном режиме, вызовы инструментов.
    pub async fn chat(
        &self,
        config: &LlmConfig,
        messages: &[Message],
        tools: &[serde_json::Value],
    ) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
        // Адрес обязателен: дефолтной LLM из env больше нет — url, ключ и
        // модель живут в подключении (выбранном агентом или дефолтном).
        let api_url = config.api_url.as_deref().ok_or_else(|| {
            "LLM не настроена: у агента нет подключения и дефолтная LLM не выбрана".to_string()
        })?;

        let request = LlmRequest {
            model: config.model.clone().unwrap_or_default(),
            messages,
            temperature: config.temperature,
            max_tokens: Some(2048),
            tools,
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
            .into_iter()
            .next()
            .map(|c| c.message)
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
            native_tools: true,
        }
    }

    /// Мок LLM-эндпоинта: записывает путь, Authorization и тело запроса,
    /// отвечает фиксированным сообщением `reply`.
    type RecordedCalls =
        std::sync::Arc<tokio::sync::Mutex<Vec<(String, Option<String>, serde_json::Value)>>>;
    async fn mock_llm_server(
        reply: serde_json::Value,
    ) -> (String, tokio::task::JoinHandle<()>, RecordedCalls) {
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
                let reply = reply.clone();
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
                    recorded.lock().await.push((uri, auth, body));
                    axum::Json(serde_json::json!({ "choices": [{ "message": reply }] }))
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
        let (api_url, server, recorded) =
            mock_llm_server(serde_json::json!({"role": "assistant", "content": "ok"})).await;
        let client = LlmClient::new();
        let mut cfg = llm_config("qwen3:0.6b", Some(&api_url));
        cfg.api_key = Some("conn-key".to_string());
        let resp = client
            .chat(&cfg, &[Message::system("sys"), Message::user("hello")], &[])
            .await
            .unwrap();
        assert_eq!(resp.content, "ok");
        let rec = recorded.lock().await;
        assert_eq!(rec.len(), 1);
        // Запрос ушёл на url подключения, с его ключом и его моделью.
        assert_eq!(rec[0].0, "/v1/chat/completions");
        assert_eq!(rec[0].1.as_deref(), Some("Bearer conn-key"));
        assert_eq!(rec[0].2["model"], "qwen3:0.6b");
        // Без инструментов поле tools не отправляется (текстовый режим).
        assert!(rec[0].2.get("tools").is_none());
        server.abort();
    }

    #[tokio::test]
    async fn chat_sends_tools_and_parses_tool_calls() {
        // Ollama отдаёт arguments объектом, OpenAI — строкой: оба читаются строкой.
        let (api_url, server, recorded) = mock_llm_server(serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [
                {"id": "c1", "type": "function",
                 "function": {"name": "shell", "arguments": "{\"command\":\"ls\"}"}},
                {"id": "c2", "type": "function",
                 "function": {"name": "shell", "arguments": {"command": "pwd"}}}
            ]
        }))
        .await;
        let client = LlmClient::new();
        let cfg = llm_config("m", Some(&api_url));
        let tools = vec![serde_json::json!({"type": "function", "function": {"name": "shell"}})];
        let resp = client
            .chat(&cfg, &[Message::user("hi")], &tools)
            .await
            .unwrap();
        assert_eq!(resp.content, "");
        assert_eq!(resp.tool_calls.len(), 2);
        assert_eq!(resp.tool_calls[0].function.arguments, r#"{"command":"ls"}"#);
        let args: serde_json::Value =
            serde_json::from_str(&resp.tool_calls[1].function.arguments).unwrap();
        assert_eq!(args["command"], "pwd");
        let rec = recorded.lock().await;
        assert_eq!(rec[0].2["tools"][0]["function"]["name"], "shell");
        server.abort();
    }

    #[test]
    fn tool_result_message_serializes_with_call_id() {
        let v = serde_json::to_value(Message::tool("c1", "out")).unwrap();
        assert_eq!(v["role"], "tool");
        assert_eq!(v["tool_call_id"], "c1");
        assert_eq!(v["content"], "out");
        assert!(v.get("tool_calls").is_none());
    }

    #[tokio::test]
    async fn chat_without_configured_llm_fails() {
        let client = LlmClient::new();
        let cfg = llm_config("qwen3:0.6b", None);
        let err = client
            .chat(&cfg, &[Message::user("hello")], &[])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("LLM не настроена"));
    }
}
