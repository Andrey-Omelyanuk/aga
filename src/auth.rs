use axum::http::HeaderMap;

use crate::chat::ChatStore;

/// Разрешить текущего пользователя из заголовков и конфига SSO.
///
/// Минимальный режим: пока SSO не включён (или токен не передан) — все запросы
/// идут под аноним-суперпользователем. Если SSO включён и передан Bearer-токен —
/// извлекаем `sub` и возвращаем (создавая при необходимости) учётку человека.
pub async fn resolve_user(headers: &HeaderMap, chat_store: &ChatStore, sso_enabled: bool) -> i64 {
    if sso_enabled {
        if let Some(sub) = bearer_subject(headers) {
            if let Ok(Some(user)) = chat_store.find_user_by_sso(&sub).await {
                return user.id;
            }
            if let Ok(id) = chat_store
                .insert_user(&sub, "human", false, Some(&sub), None)
                .await
            {
                return id;
            }
        }
    }

    // Аноним-суперпользователь (первая учётка kind = 'anonymous').
    chat_store.anonymous_id().await.unwrap_or(1)
}

/// Извлечь `sub` из Bearer JWT (без проверки подписи — минимально).
fn bearer_subject(headers: &HeaderMap) -> Option<String> {
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    let payload = token.split('.').nth(1)?;
    let decoded = base64url_decode(payload)?;
    let json: serde_json::Value = serde_json::from_str(&decoded).ok()?;
    json.get("sub")?.as_str().map(|s| s.to_string())
}

fn base64url_decode(input: &str) -> Option<String> {
    use base64::Engine;
    let padded = input.replace('-', "+").replace('_', "/");
    let b64 = match padded.len() % 4 {
        0 => padded,
        2 => format!("{padded}=="),
        3 => format!("{padded}="),
        _ => return None,
    };
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    Some(String::from_utf8_lossy(&bytes).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_jwt_subject() {
        // { "sub": "andrey", "name": "x" }
        let p = base64url_encode(r#"{"sub":"andrey","name":"x"}"#);
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer eyJh.{}..", p).parse().unwrap(),
        );
        assert_eq!(bearer_subject(&headers), Some("andrey".to_string()));
    }

    fn base64url_encode(s: &str) -> String {
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s.as_bytes())
    }
}
