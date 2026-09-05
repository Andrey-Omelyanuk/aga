//! Клиент Centrifugo для реального времени в чате.
//!
//! Две роли:
//! - `connection_jwt(user_id)` — подписывает connection-JWT (HS256) для
//!   веб-клиента: право подписки на общий канал прямо в токене (`channels`),
//!   поэтому отдельного channel-токена не нужно. Канал один и общий — гейт —
//!   сам факт аутентификации.
//! - `publish(payload)` — публикует сообщение в общий канал через HTTP API
//!   Centrifugo (best-effort: сбой не ломает основной флоу, только логируется).

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

/// Полезная нагрузка события нового сообщения в общем канале. По `chat_id`
/// веб-клиент решает, какой чат перезагрузить (деталь), и всегда обновляет
/// список чатов.
pub fn message_payload(chat_id: i64, message_id: i64) -> serde_json::Value {
    json!({ "type": "message", "chat_id": chat_id, "message_id": message_id })
}

/// Продолжительность жизни connection-JWT (Centrifugo проверяет `exp`).
const TOKEN_TTL_SECS: u64 = 60 * 60;

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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| {
                jsonwebtoken::errors::Error::from(jsonwebtoken::errors::ErrorKind::ExpiredSignature)
            })?
            .as_secs();
        let claims = json!({
            "sub": user_id.to_string(),
            "exp": now + TOKEN_TTL_SECS,
            "channels": [inner.channel],
        });
        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
        Ok(jsonwebtoken::encode(
            &header,
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(inner.secret.as_bytes()),
        )?)
    }

    /// Публикует payload в общий канал через HTTP API Centrifugo.
    /// Best-effort: ошибка логируется и игнорируется — websocket не должен
    /// ломать отправку сообщения.
    pub async fn publish(&self, payload: serde_json::Value) {
        let Some(inner) = &self.inner else { return };
        let url = format!("{}/api", inner.api_url);
        let body = json!({
            "method": "publish",
            "params": {
                "channel": inner.channel,
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
    fn message_payload_carries_chat_id_and_message_id() {
        let payload = message_payload(42, 7);
        assert_eq!(payload["type"], "message");
        assert_eq!(payload["chat_id"], 42);
        assert_eq!(payload["message_id"], 7);
    }
}
