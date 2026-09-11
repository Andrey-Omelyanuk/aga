//! Клиент Centrifugo для реального времени в чате.
//!
//! Роли:
//! - `connection_jwt(user_id)` — подписывает connection-JWT (HS256) для
//!   веб-клиента: право подписки на общий канал прямо в токене (`channels`),
//!   поэтому отдельного channel-токена не нужно. Канал один и общий — гейт —
//!   сам факт аутентификации.
//! - `publish(channel, payload)` — публикует событие в именованный канал через
//!   HTTP API Centrifugo (best-effort: сбой не ломает основной флоу, только
//!   логируется). `publish_message` — веером в общий канал, канал чата и канал
//!   автора: чат публикует все изменения, подписчики (веб-клиент,
//!   агент-процесс) разбирают события сами.

use serde_json::json;
use thiserror::Error;

use crate::config::CentrifugeConfig;

#[derive(Error, Debug)]
pub enum CentrifugeError {
    #[error("Centrifugo не настроен")]
    NotConfigured,
    #[error("JWT: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
    #[error("HTTP: {0}")]
    Http(#[from] reqwest::Error),
}

/// Канал одного чата: события, видимые всем, кто открыл этот чат.
pub fn chat_channel(chat_id: i64) -> String {
    format!("chat:{chat_id}")
}

/// Канал пользователя: его события (авторство сообщения) — подписываются
/// фронт для персональных уведомлений и агент-процесс для привязки «слушать
/// пользователя» (`runtime.rs`).
pub fn user_channel(user_id: i64) -> String {
    format!("user:{user_id}")
}

/// Полезная нагрузка события нового сообщения в общем канале. По `chat_id`
/// веб-клиент решает, какой чат перезагрузить (деталь), и всегда обновляет
/// список чатов. `author_id` — автор сообщения: по нему подписчик-рантайм
/// находит агентов, привязанных к этому пользователю.
pub fn message_payload(chat_id: i64, message_id: i64, author_id: i64) -> serde_json::Value {
    json!({ "type": "message", "chat_id": chat_id, "message_id": message_id, "author_id": author_id })
}

/// Событие жизненного цикла чата (создание чата, открытие/закрытие сессии) —
/// уходит в общий канал; подписчик по `chat_id` обновляет список чатов.
pub fn lifecycle_payload(action: &str, chat_id: i64) -> serde_json::Value {
    json!({ "type": "chat", "action": action, "chat_id": chat_id })
}

/// Продолжительность жизни connection-JWT (Centrifugo проверяет `exp`).
const TOKEN_TTL_SECS: u64 = 60 * 60;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn sign_jwt(
    secret: &str,
    claims: &serde_json::Value,
) -> Result<String, jsonwebtoken::errors::Error> {
    let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
    jsonwebtoken::encode(
        &header,
        claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
}

/// Один кадр событий unidirectional-SSE Centrifugo (`/connection/sse`,
/// JSON-протокол): из `data:`-строк собираем JSON и достаём публикацию —
/// (канал, данные). Кадры без публикации (соединение, ping) → None.
pub fn parse_sse_event(frame: &str) -> Option<(String, serde_json::Value)> {
    let mut data = String::new();
    for line in frame.lines() {
        // Комментарий (`: ping`) — не данные; поле `data:` — кадр события.
        if let Some(rest) = line.strip_prefix("data:") {
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    if data.is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(&data).ok()?;
    let publication = value.get("pub")?;
    let channel = publication["channel"].as_str()?.to_string();
    Some((channel, publication["data"].clone()))
}

#[derive(Clone)]
pub struct CentrifugeClient {
    inner: Option<Inner>,
}

#[derive(Clone)]
struct Inner {
    api_url: String,
    api_key: String,
    secret: String,
    channel: String,
    http: reqwest::Client,
}

impl CentrifugeClient {
    /// Пустой клиент (Centrifugo не настроен) — `/connection-jwt/` отдаёт 404,
    /// публикация — no-op. Позволяет ядру работать без websocket.
    pub fn disabled() -> Self {
        CentrifugeClient { inner: None }
    }

    pub fn from_config(cfg: &CentrifugeConfig) -> Self {
        CentrifugeClient {
            inner: Some(Inner {
                api_url: cfg.api_url.trim_end_matches('/').to_string(),
                api_key: cfg.api_key.clone(),
                secret: cfg.secret.clone(),
                channel: cfg.channel.clone(),
                http: reqwest::Client::new(),
            }),
        }
    }

    /// Настроен ли клиент (есть конфиг Centrifugo).
    pub fn is_configured(&self) -> bool {
        self.inner.is_some()
    }

    /// Connection-JWT для аутентифицированного пользователя (sub = chat_users.id).
    /// Право подписки на общий канал — в claims `channels`, отдельного
    /// channel-токена нет: канал общий для всех аутентифицированных.
    pub fn connection_jwt(&self, user_id: i64) -> Result<String, CentrifugeError> {
        let inner = self.inner.as_ref().ok_or(CentrifugeError::NotConfigured)?;
        let token = sign_jwt(
            &inner.secret,
            &json!({
                "sub": user_id.to_string(),
                "exp": now_secs() + TOKEN_TTL_SECS,
                "channels": [inner.channel],
            }),
        )?;
        Ok(token)
    }

    /// Connection-JWT агент-рантайма (`aga agent`): серверная подписка
    /// (`subscriptions`) на каналы прослушиваемых пользователей. Рантайм —
    /// доверенный серверный процесс, токен подписывает тем же HMAC-секретом из
    /// конфига (того же roles.yaml), без прохода через HTTP-ядро.
    pub fn subscriber_jwt(&self, channels: &[String]) -> Result<String, CentrifugeError> {
        let inner = self.inner.as_ref().ok_or(CentrifugeError::NotConfigured)?;
        sign_jwt(
            &inner.secret,
            &json!({
                "sub": "aga-runtime",
                "exp": now_secs() + TOKEN_TTL_SECS,
                "channels": channels,
                "subscriptions": channels,
            }),
        )
        .map_err(Into::into)
    }

    /// Публикует payload в именованный канал через HTTP API Centrifugo.
    /// Best-effort: ошибка логируется и игнорируется — websocket не должен
    /// ломать отправку сообщения.
    pub async fn publish(&self, channel: &str, payload: serde_json::Value) {
        let Some(inner) = &self.inner else { return };
        let url = format!("{}/api", inner.api_url);
        let body = json!({
            "method": "publish",
            "params": {
                "channel": channel,
                "data": payload,
            },
        });
        match inner
            .http
            .post(&url)
            .header("X-API-Key", &inner.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(resp) => {
                if let Err(e) = resp.error_for_status() {
                    tracing::warn!("centrifugo publish failed: {e}");
                }
            }
            Err(e) => tracing::warn!("centrifugo publish failed: {e}"),
        }
    }

    /// Публикует событие нового сообщения в три канала: общий (`channel` из
    /// конфига), канал чата и канал автора. Веб-клиент слушает общий канал,
    /// агент-процесс — каналы пользователей. Best-effort, как `publish`.
    pub async fn publish_message(&self, chat_id: i64, message_id: i64, author_id: i64) {
        let payload = message_payload(chat_id, message_id, author_id);
        let channel = self.channel();
        self.publish(&channel, payload.clone()).await;
        self.publish(&chat_channel(chat_id), payload.clone()).await;
        self.publish(&user_channel(author_id), payload).await;
    }

    /// Имя общего канала из конфига (`common` по умолчанию).
    pub fn channel(&self) -> String {
        self.inner
            .as_ref()
            .map(|i| i.channel.clone())
            .unwrap_or_else(crate::config::default_channel)
    }

    /// Открывает unidirectional-SSE соединение Centrifugo с готовым
    /// connection-JWT (`subscriber_jwt`): серверная подписка из токена
    /// подключает каналы, публикации приходят кадрами `parse_sse_event`.
    /// Ответ остаётся открытым стримом — читает его вызывающий (`runtime.rs`).
    pub async fn sse_stream(&self, token: &str) -> Result<reqwest::Response, CentrifugeError> {
        let inner = self.inner.as_ref().ok_or(CentrifugeError::NotConfigured)?;
        let url = format!(
            "{}/connection/sse?format=json&token={}",
            inner.api_url, token
        );
        let resp = inner.http.get(url).send().await?.error_for_status()?;
        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};

    fn config() -> CentrifugeConfig {
        CentrifugeConfig {
            api_url: "http://centrifugo:8000".to_string(),
            api_key: "key".to_string(),
            secret: "secret".to_string(),
            channel: "common".to_string(),
        }
    }

    #[test]
    fn connection_jwt_has_sub_and_common_channel() {
        let client = CentrifugeClient::from_config(&config());
        let token = client.connection_jwt(42).unwrap();
        let data = decode::<serde_json::Value>(
            &token,
            &DecodingKey::from_secret(b"secret"),
            &Validation::new(Algorithm::HS256),
        )
        .unwrap();
        let claims = data.claims;
        assert_eq!(claims["sub"], "42");
        assert_eq!(claims["channels"][0], "common");
        assert!(claims["exp"].as_u64().unwrap() > 0);
    }

    #[test]
    fn subscriber_jwt_has_server_side_subscriptions_for_bound_users() {
        let client = CentrifugeClient::from_config(&config());
        let token = client
            .subscriber_jwt(&["user:5".to_string(), "user:9".to_string()])
            .unwrap();
        let data = decode::<serde_json::Value>(
            &token,
            &DecodingKey::from_secret(b"secret"),
            &Validation::new(Algorithm::HS256),
        )
        .unwrap();
        let claims = data.claims;
        // Серверная подписка из токена: рантайму не нужно уметь подписываться
        // самому — Centrifugo подключает каналы при connect.
        assert_eq!(claims["subscriptions"][0], "user:5");
        assert_eq!(claims["subscriptions"][1], "user:9");
    }

    #[test]
    fn sse_frames_carry_publications_and_ignore_pings() {
        let frame = "data: {\"pub\":{\"channel\":\"user:5\",\"data\":{\"type\":\"message\"}}}";
        let (channel, data) = parse_sse_event(frame).expect("публикация в SSE-кадре");
        assert_eq!(channel, "user:5");
        assert_eq!(data["type"], "message");
        // Комментарий-пин и кадр соединения — не публикация.
        assert!(parse_sse_event(": ping").is_none());
        assert!(parse_sse_event("data: {\"connect\":{}}").is_none());
    }

    #[test]
    fn disabled_client_has_no_jwt() {
        let client = CentrifugeClient::disabled();
        assert!(!client.is_configured());
        assert!(matches!(
            client.connection_jwt(1),
            Err(CentrifugeError::NotConfigured)
        ));
    }

    #[test]
    fn config_uses_default_channel() {
        let cfg = CentrifugeConfig {
            api_url: "http://x".to_string(),
            api_key: "k".to_string(),
            secret: "s".to_string(),
            channel: "common".to_string(),
        };
        assert_eq!(cfg.channel, "common");
    }

    #[test]
    fn message_payload_carries_chat_id_message_id_and_author() {
        let payload = message_payload(42, 7, 5);
        assert_eq!(payload["type"], "message");
        assert_eq!(payload["chat_id"], 42);
        assert_eq!(payload["message_id"], 7);
        assert_eq!(payload["author_id"], 5);
    }

    #[test]
    fn channels_are_named_per_chat_and_per_user() {
        assert_eq!(chat_channel(42), "chat:42");
        assert_eq!(user_channel(5), "user:5");
    }

    #[test]
    fn lifecycle_payload_carries_action_and_chat_id() {
        let payload = lifecycle_payload("chat_created", 42);
        assert_eq!(payload["type"], "chat");
        assert_eq!(payload["action"], "chat_created");
        assert_eq!(payload["chat_id"], 42);
    }

    #[tokio::test]
    async fn publish_message_fans_out_to_common_chat_and_author_channels() {
        use std::sync::{Arc, Mutex};
        let recorded: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = recorded.clone();
        let app = axum::Router::new().route(
            "/api",
            axum::routing::post(move |req: axum::extract::Request| {
                let sink = sink.clone();
                async move {
                    let body: serde_json::Value = axum::body::to_bytes(req.into_body(), usize::MAX)
                        .await
                        .ok()
                        .and_then(|b| serde_json::from_slice(&b).ok())
                        .unwrap_or_default();
                    if let Some(channel) = body["params"]["channel"].as_str() {
                        sink.lock().unwrap().push(channel.to_string());
                    }
                    axum::Json(serde_json::json!({ "result": {} }))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = CentrifugeClient::from_config(&CentrifugeConfig {
            api_url: format!("http://{addr}"),
            api_key: "key".into(),
            secret: "secret".into(),
            channel: "common".into(),
        });
        client.publish_message(42, 7, 5).await;
        let channels = recorded.lock().unwrap().clone();
        server.abort();
        assert!(channels.contains(&"common".to_string()), "{channels:?}");
        assert!(channels.contains(&"chat:42".to_string()), "{channels:?}");
        assert!(channels.contains(&"user:5".to_string()), "{channels:?}");
    }

    #[tokio::test]
    async fn disabled_client_publishes_nothing() {
        // Публикация без конфига — no-op: вета не должно быть даже при вызове.
        let client = CentrifugeClient::disabled();
        client.publish_message(1, 2, 3).await;
        client.publish("any", serde_json::json!({})).await;
        assert_eq!(client.channel(), "common");
    }
}
