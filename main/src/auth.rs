use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;

use crate::chat::ChatStore;

/// Верификатор JWT против JWKS (RS256). Собран один раз при старте;
/// подпись токена проверяется настоящая (Keycloak).
#[derive(Debug, Clone)]
pub struct JwtVerifier {
    keys: serde_json::Value,
}

/// Полезная нагрузка JWT от Keycloak.
#[derive(Debug, Deserialize)]
pub struct Claims {
    pub sub: String,
    #[serde(default)]
    pub realm_access: Option<RealmAccess>,
}

#[derive(Debug, Deserialize)]
pub struct RealmAccess {
    #[serde(default)]
    pub roles: Vec<String>,
}

impl JwtVerifier {
    /// Собрать верификатор из JWKS-документа (JSON).
    pub fn from_jwks_json(jwks: &str) -> Result<Self, serde_json::Error> {
        let keys: serde_json::Value = serde_json::from_str(jwks)?;
        Ok(Self { keys })
    }

    /// Проверить подпись токена RS256 и вернуть claims. Недействительный
    /// токен (плохая подпись, нет ключа по kid, истёк) — ошибка.
    pub fn verify(&self, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
        let header = jsonwebtoken::decode_header(token)?;
        let keys = self
            .keys
            .get("keys")
            .and_then(|k| k.as_array())
            .cloned()
            .unwrap_or_default();
        let key = keys
            .iter()
            .find(|k| k.get("kid").and_then(|v| v.as_str()) == header.kid.as_deref())
            .or_else(|| keys.first())
            .ok_or(jsonwebtoken::errors::Error::from(
                jsonwebtoken::errors::ErrorKind::InvalidToken,
            ))?;
        // RSA-only поддержка (Keycloak по умолчанию RS256). n/e base64url из JWK.
        let n = key.get("n").and_then(|v| v.as_str()).ok_or_else(|| {
            jsonwebtoken::errors::Error::from(jsonwebtoken::errors::ErrorKind::InvalidKeyFormat)
        })?;
        let e = key.get("e").and_then(|v| v.as_str()).ok_or_else(|| {
            jsonwebtoken::errors::Error::from(jsonwebtoken::errors::ErrorKind::InvalidKeyFormat)
        })?;
        let decoding_key = jsonwebtoken::DecodingKey::from_rsa_components(n, e)?;
        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
        // Не требуем issuer/audience — минимальная проверка подписи и sub.
        validation.validate_aud = false;
        validation.validate_exp = true;
        let data = jsonwebtoken::decode::<Claims>(token, &decoding_key, &validation)?;
        Ok(data.claims)
    }
}

/// Разрешить текущего пользователя из заголовков и конфига SSO.
///
/// Без SSO — аноним-суперпользователь (локальный режим). С SSO — токен
/// обязателен и проверяется по JWKS; из `sub` берётся (создаётся) учётка
/// человека-участника. Недействительный/отсутствующий токен — 401.
pub async fn resolve_user(
    headers: &HeaderMap,
    chat_store: &ChatStore,
    sso_verifier: Option<&JwtVerifier>,
) -> Result<i64, StatusCode> {
    match sso_verifier {
        None => Ok(chat_store.anonymous_id().await.unwrap_or(1)),
        Some(verifier) => {
            let token = token_from(headers).ok_or(StatusCode::UNAUTHORIZED)?;
            let claims = verifier
                .verify(&token)
                .map_err(|_| StatusCode::UNAUTHORIZED)?;
            let is_admin = claims
                .realm_access
                .as_ref()
                .map(|r| r.roles.iter().any(|r| r == "admin"))
                .unwrap_or(false);
            if let Some(user) = chat_store
                .find_user_by_sso(&claims.sub)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            {
                // Роль в Keycloak могла измениться — обновляем флаг суперпользователя.
                let _ = chat_store.set_super_user(user.id, is_admin).await;
                return Ok(user.id);
            }
            chat_store
                .insert_user(&claims.sub, "human", is_admin, Some(&claims.sub), None)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Извлечь Bearer-токен из заголовка Authorization.
fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    auth.strip_prefix("Bearer ").map(|s| s.to_string())
}

/// Токен из Authorization Bearer или cookie `aga_token` (веб-клиент после
/// логина через Keycloak шлёт токен cookie).
fn token_from(headers: &HeaderMap) -> Option<String> {
    if let Some(t) = bearer_token(headers) {
        return Some(t);
    }
    let cookie = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for part in cookie.split(';') {
        if let Some(v) = part.trim().strip_prefix("aga_token=") {
            return Some(v.to_string());
        }
    }
    None
}

#[cfg(test)]
pub(crate) const TEST_PRIV_PEM: &str = "-----BEGIN PRIVATE KEY-----\nMIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDMJ+/HsZNAy08B\nA7sxG/td1C9whtiFgtjwPlptwsFAMUYicPFWv0pk3P3jxd87/KRs9QQ3Q5/RQbIk\noyzS114+ximSMp25S3GP4Aua4s5bMSwuktIotxQOaT2D6cQYrLjb8tnCvccjAT0m\n680YHi8F0vVnv+7pB48kYhBRvJhGVg41hJ7o5uOYk77OW9pkd3MFGIKU8tYbyqGd\nFUebLSvl3ew4K5y0mRT5QmqeGdwespj2GwRHp1Jx2/t4w+mg7lbkMHgOw3f2F9lm\n0dNNNhQiIB08O79EmVkjwI9Z4OJwv+GLk8+H+FtJRoLg8uNdtp73+OVAe2iJWa8p\nOI18/ysFAgMBAAECggEADN7Bp3mi2lVWzC4TiXOFo7MiMHpXwQbwLSkJI11BOI5C\nqR4soLbbdkNWQBszyQDSzsmdc+xv8U2ucM/Wng4Us2ljqoNFXS6L4LGTmbaNosMV\nUXRRCl8MRSJOTfgZNCMDXl5Paw7ytFq6I69+1PPmK/xSGzHG2mrvE7CY7cYxZVGf\nrPE3iURbZeBJt8KGAyP/QBcJwzNtQo1xWNpP25qgkl3uJiHnJXb1V8Ae6cg5Sisz\ntU6yMJCMk1+5B7NOC+UVp9Dxgxu8u9wJNchKKh4oK0jVbj3Hj8rkXIgk817WrQlr\no/uAai+s8nOF5favsIPVDIJMjHkdZN0sAvXLPUkhuwKBgQDn2AXt2TGPGBjKPT08\nMkPA0DBtwWuqHCrvsZx/B99QEIeKYo63dhWXhn4QGci0Qd4mwXD/N1Grx2VchRmE\ne0C/3FVNOgHHpTdS1KIcdayZjFQa4qKVz51N4uNIAv2fs7jJoqOZdukJ2/LhKy/O\nED8PnskPcrvwlQir39WZOM0LMwKBgQDhbWUiHSFCZO0dxO/JvvNC6nCwFzX4yZRc\nq+NQ1lFpI+gGb2bYtHQimQAbUhZpDeexeCXSbIVKt7R3RPC1MOKgjFOqOUaryWDY\nvsTM6A+sQIkjR1ZQ7EQg2ikPepdqR0rG5jvvsjBd93mX48OpkUi+M8VJSSsDdx2/\nBrn4Ap2w5wKBgQCMPNVJS+l4XuEP4/8YXGczSDsjCK5xVVx7ZHn/NOnVakoyYO9m\n9dyVrVqvrokC0BzqYHRTTEjwmUosrq4CvvMpmsNWVVIiS0OtrMTqZhujPYjaQmCK\nMe064ZUNSBHV+kY6YVCIUa8gsZS2swLVqGocrrV7zLD2E5ANNvXjGsKclQKBgQC+\nWWxbWPObp7NdPs0nstigeWv8FS1azYQ8mFwTB1WpDUvAG1Nhy0aBbGZdq3wG61no\nTkbJnx8ST3rQd2M17HiBDt0a0NBvAFWJz9RIHfAWCEyEgJlPLaH9h5nCW0b91AM9\nXm3f4bvbrLt82TN/vJELIpYFYwYyH+P7SMfBtxvGowKBgEhMr6Q9al/GNluyEU5y\nI7JiA2psYe4t2kZSP7cINFA4zFrBzdORukiNdPuS4uw5Vz/mljg8rzwhwamJ0AQT\n0hyA5h8RnHQ8vFYpY0NtVvT6tnWFBPqtaQbugUbU9/wOlh25Pwfqgur32qkfkmgq\nSFpuWNa2eg+nXHhxKWNJQG+n\n-----END PRIVATE KEY-----\n";

#[cfg(test)]
pub(crate) const TEST_JWKS: &str = r#"{"keys":[{"kty":"RSA","kid":"test-key","use":"sig","alg":"RS256","n":"zCfvx7GTQMtPAQO7MRv7XdQvcIbYhYLY8D5abcLBQDFGInDxVr9KZNz948XfO_ykbPUEN0Of0UGyJKMs0tdePsYpkjKduUtxj-ALmuLOWzEsLpLSKLcUDmk9g-nEGKy42_LZwr3HIwE9JuvNGB4vBdL1Z7_u6QePJGIQUbyYRlYONYSe6ObjmJO-zlvaZHdzBRiClPLWG8qhnRVHmy0r5d3sOCuctJkU-UJqnhncHrKY9hsER6dScdv7eMPpoO5W5DB4DsN39hfZZtHTTTYUIiAdPDu_RJlZI8CPWeDicL_hi5PPh_hbSUaC4PLjXbae9_jlQHtoiVmvKTiNfP8rBQ","e":"AQAB"}]}"#;

#[cfg(test)]
pub(crate) fn test_sign_token(sub: &str, roles: &[&str]) -> String {
    use jsonwebtoken::{Algorithm, EncodingKey, Header};
    let exp = (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp();
    let claims = serde_json::json!({
        "sub": sub,
        "exp": exp,
        "realm_access": { "roles": roles },
    });
    jsonwebtoken::encode(
        &Header::new(Algorithm::RS256),
        &claims,
        &EncodingKey::from_rsa_pem(TEST_PRIV_PEM.as_bytes()).unwrap(),
    )
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Тестовый RSA-ключ: подпись токена (приватный) и JWKS (публичный).

    const JWKS: &str = TEST_JWKS;

    fn verifier() -> JwtVerifier {
        JwtVerifier::from_jwks_json(JWKS).unwrap()
    }

    fn sign_token(sub: &str, roles: &[&str]) -> String {
        test_sign_token(sub, roles)
    }

    #[test]
    fn verifies_valid_token_and_extracts_sub_and_roles() {
        let v = verifier();
        let claims = v.verify(&sign_token("andrey", &["participant"])).unwrap();
        assert_eq!(claims.sub, "andrey");
        assert_eq!(
            claims.realm_access.unwrap().roles,
            vec!["participant".to_string()]
        );
    }

    #[test]
    fn rejects_invalid_token() {
        let v = verifier();
        // Испорченная подпись.
        let mut token = sign_token("andrey", &["participant"]);
        token.pop();
        token.push('x');
        assert!(v.verify(&token).is_err());
    }

    #[test]
    fn rejects_tampered_payload() {
        use base64::Engine;
        let v = verifier();
        let token = sign_token("andrey", &["participant"]);
        let mut parts: Vec<String> = token.split('.').map(|s| s.to_string()).collect();
        let tampered_payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"sub":"evil","exp":9999999999}"#);
        parts[1] = tampered_payload;
        let tampered = parts.join(".");
        assert!(v.verify(&tampered).is_err());
    }

    async fn chat_store() -> (crate::chat::ChatStore, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("aga_auth_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = crate::chat::ChatStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        (store, path)
    }

    #[tokio::test]
    async fn participant_resolves_from_valid_token() {
        let (store, file) = chat_store().await;
        let v = verifier();
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", sign_token("andrey", &["participant"]))
                .parse()
                .unwrap(),
        );
        let id = resolve_user(&headers, &store, Some(&v)).await.unwrap();
        let user = store.get_user(id).await.unwrap().unwrap();
        assert_eq!(user.kind, "human");
        assert!(!user.is_super_user);
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn invalid_token_rejected() {
        let (store, file) = chat_store().await;
        let v = verifier();
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer not.a.jwt".parse().unwrap(),
        );
        assert_eq!(
            resolve_user(&headers, &store, Some(&v)).await.unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn anonymous_superuser_without_sso() {
        let (store, file) = chat_store().await;
        let headers = HeaderMap::new();
        let id = resolve_user(&headers, &store, None).await.unwrap();
        assert!(store.is_super_user(id).await.unwrap());
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn admin_role_maps_to_super_user() {
        let (store, file) = chat_store().await;
        let v = verifier();
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", sign_token("boss", &["admin", "participant"]))
                .parse()
                .unwrap(),
        );
        let id = resolve_user(&headers, &store, Some(&v)).await.unwrap();
        assert!(store.is_super_user(id).await.unwrap());
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
        let _ = std::fs::remove_file(&file);
    }
}
