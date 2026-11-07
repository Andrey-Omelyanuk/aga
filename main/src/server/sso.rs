//! SSO (Keycloak): вход веб-клиента — `/auth/login`, `/auth/callback`,
//! `/auth/refresh`, `/auth/logout` — и помощники флоу (redirect_uri, cookie
//! `state`+PKCE, атрибут `Secure`).

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use serde::Deserialize;

use super::{AppError, AppState};

// === SSO (Keycloak): вход веб-клиента ===

/// Абсолютный `redirect_uri` для SSO-флоу: `{scheme}://{host}/auth/callback`.
/// scheme берётся из `X-Forwarded-Proto` (когда запрос идёт через ingress/прокси),
/// иначе http; host — из заголовка `Host`. Относительный redirect_uri Keycloak
/// резолвит в свой собственный хост (auth.localhost) — после входа браузер
/// попадает на Keycloak и получает 404 "Page not found". Поэтому хост обязателен.
fn sso_redirect_uri(headers: &HeaderMap) -> Option<String> {
    let host = headers.get(axum::http::header::HOST)?.to_str().ok()?;
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    Some(format!("{scheme}://{host}/auth/callback"))
}

/// Cookie с `state` и PKCE-верификатором между `/auth/login` и
/// `/auth/callback`: HttpOnly, только для пути колбэка, живёт 10 минут.
const OAUTH_COOKIE: &str = "aga_oauth";
const OAUTH_COOKIE_TTL_SECS: u64 = 600;

/// Атрибут `Secure` для кук, когда ядро снаружи доступно по https (прокси
/// передаёт `X-Forwarded-Proto`): по http-стенду браузер такую куку не вернёт.
pub(super) fn cookie_secure_attr(redirect_uri: &str) -> &'static str {
    if redirect_uri.starts_with("https://") {
        "; Secure"
    } else {
        ""
    }
}

/// PKCE S256: `code_challenge = BASE64URL(SHA256(code_verifier))` без паддинга.
pub(super) fn pkce_challenge(verifier: &str) -> String {
    use base64::Engine;
    use sha2::Digest;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()))
}

/// `state` и PKCE-верификатор, которые `/auth/login` положил в cookie.
fn oauth_cookie(headers: &HeaderMap) -> Option<(String, String)> {
    let cookie = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    let value = cookie
        .split(';')
        .find_map(|part| part.trim().strip_prefix(&format!("{OAUTH_COOKIE}=")))?;
    let (state, verifier) = value.split_once('.')?;
    (!state.is_empty() && !verifier.is_empty()).then(|| (state.to_string(), verifier.to_string()))
}

/// Начать вход: редирект на authorize-эндпоинт Keycloak (authorization code +
/// PKCE S256). Случайный `state` и верификатор PKCE уходят в HttpOnly-cookie
/// `aga_oauth` — колбэк сверяет `state` (защита от login-CSRF: чужой code не
/// подсунуть) и отдаёт верификатор Keycloak при обмене кода. Токен после входа
/// возвращается веб-клиенту (front_url/#token=...) — см. auth_callback.
pub(super) async fn auth_login(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let sso = state
        .config
        .sso
        .as_ref()
        .filter(|s| s.enabled)
        .ok_or(StatusCode::NOT_FOUND)?;
    let authorize_url = sso.authorize_url.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    let client_id = sso.client_id.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    // Куда Keycloak вернёт после входа — абсолютный адрес из заголовка Host
    // (Origin при обычной навигации браузер не шлёт). Список допустимых
    // адресов держит Keycloak (redirectUris клиента в realm).
    let redirect_uri = sso_redirect_uri(&headers).ok_or(StatusCode::NOT_FOUND)?;
    let oauth_state = uuid::Uuid::new_v4().simple().to_string();
    // 64 символа [0-9a-f] — в пределах 43..128 из RFC 7636.
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let mut url = url::Url::parse(authorize_url).map_err(|_| StatusCode::NOT_FOUND)?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", "openid")
        .append_pair("state", &oauth_state)
        .append_pair("code_challenge", &pkce_challenge(&verifier))
        .append_pair("code_challenge_method", "S256");
    let cookie = format!(
        "{OAUTH_COOKIE}={oauth_state}.{verifier}; Path=/auth/callback; HttpOnly; SameSite=Lax; Max-Age={OAUTH_COOKIE_TTL_SECS}{}",
        cookie_secure_attr(&redirect_uri)
    );
    let mut resp = Redirect::temporary(url.as_str()).into_response();
    resp.headers_mut()
        .insert(axum::http::header::SET_COOKIE, cookie.parse()?);
    Ok(resp)
}

/// Обработчик возврата из Keycloak: сверить `state` с cookie `aga_oauth`,
/// обменять code на токен (с PKCE-верификатором) и вернуть его веб-клиенту
/// (redirect на front_url с токеном во фрагменте URL). Нет cookie или `state`
/// не совпал — 400: вход начинался не в этом браузере.
pub(super) async fn auth_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Response, AppError> {
    let sso = state
        .config
        .sso
        .as_ref()
        .filter(|s| s.enabled)
        .ok_or(StatusCode::NOT_FOUND)?;
    // Keycloak вернул ошибку вместо кода (отказ, отменённый вход).
    if params.contains_key("error") {
        return Err(StatusCode::BAD_REQUEST.into());
    }
    let code = params.get("code").ok_or(StatusCode::BAD_REQUEST)?;
    let (expected_state, verifier) = oauth_cookie(&headers).ok_or(StatusCode::BAD_REQUEST)?;
    if params.get("state") != Some(&expected_state) {
        return Err(StatusCode::BAD_REQUEST.into());
    }
    let token_url = sso.token_url.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    let client_id = sso.client_id.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    let client_secret = sso.client_secret.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    // Keycloak не эхоит redirect_uri в колбэке — вычисляем из Host так же,
    // как на authorize-шаге (значения должны совпасть).
    let redirect_uri = sso_redirect_uri(&headers).ok_or(StatusCode::BAD_REQUEST)?;

    let client = reqwest::Client::new();
    let resp = client
        .post(token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &redirect_uri),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code_verifier", &verifier),
        ])
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !resp.status().is_success() {
        // invalid_grant: код уже использован, истёк или не сошёлся PKCE.
        return Err(StatusCode::BAD_REQUEST.into());
    }
    let json: serde_json::Value = resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let token = json
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or(StatusCode::BAD_GATEWAY)?;
    // Refresh-токен живёт дольше access-токена: веб-клиент обновляет им токен
    // молча (`/auth/refresh`), не завися от SSO-куки Keycloak.
    let refresh_token = json.get("refresh_token").and_then(|v| v.as_str());

    // Веб-клиент (front/) получает токены фрагментом URL — фрагмент не уходит
    // на сервер и не остаётся в логах. Refresh-токен — вторым параметром,
    // чтобы обновлять access-токен без повторного входа.
    let fragment = match refresh_token {
        Some(rt) => format!("#token={token}&refresh={rt}"),
        None => format!("#token={token}"),
    };
    let secure = cookie_secure_attr(&redirect_uri);
    let mut resp = Redirect::temporary(&format!("{}{fragment}", state.front_url)).into_response();
    // Cookie aga_token — для прямых клиентов api.localhost; cookie флоу входа
    // одноразовая — стираем.
    for cookie in [
        format!("aga_token={token}; Path=/; HttpOnly; SameSite=Lax{secure}"),
        format!("{OAUTH_COOKIE}=; Path=/auth/callback; HttpOnly; SameSite=Lax; Max-Age=0{secure}"),
    ] {
        resp.headers_mut()
            .append(axum::http::header::SET_COOKIE, cookie.parse()?);
    }
    Ok(resp)
}

/// Выход: сбрасываем cookie `aga_token` (HttpOnly — JS его сам не удалит) и,
/// если задан end-session эндпоинт Keycloak, редиректим туда (завершение
/// SSO-сессии, возврат на фронт), иначе — просто на фронт. Веб-клиент перед
/// этим удаляет токен из localStorage.
pub(super) async fn auth_logout(State(state): State<AppState>) -> Result<Response, AppError> {
    let sso = state.config.sso.as_ref().filter(|s| s.enabled);
    let target = match sso {
        Some(s) => match s.end_session_url.as_deref() {
            Some(end) => {
                let mut url = url::Url::parse(end).map_err(|_| StatusCode::NOT_FOUND)?;
                url.query_pairs_mut()
                    .append_pair("client_id", s.client_id.as_deref().unwrap_or_default())
                    .append_pair("post_logout_redirect_uri", &state.front_url);
                url.to_string()
            }
            None => state.front_url.clone(),
        },
        None => state.front_url.clone(),
    };
    let mut resp = Redirect::temporary(&target).into_response();
    resp.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        "aga_token=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0".parse()?,
    );
    Ok(resp)
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// Молча обновить access-токен: веб-клиент шлёт refresh-токен, ядро меняет его
/// у Keycloak (`grant_type=refresh_token`) и возвращает свежие токены JSON'ом
/// плюс кладёт access-токен в cookie `aga_token` (прямые api.localhost-клиенты).
/// В отличие от silent-iframe, браузерные куки и их кросс-сайтовая блокировка
/// не участвуют. Недействительный/истёкший refresh-токен — 401.
pub(super) async fn auth_refresh(
    State(state): State<AppState>,
    Json(payload): Json<RefreshRequest>,
) -> Result<Response, AppError> {
    let sso = state
        .config
        .sso
        .as_ref()
        .filter(|s| s.enabled)
        .ok_or(StatusCode::NOT_FOUND)?;
    let token_url = sso.token_url.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    let client_id = sso.client_id.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    let client_secret = sso.client_secret.as_deref().ok_or(StatusCode::NOT_FOUND)?;

    let client = reqwest::Client::new();
    let resp = client
        .post(token_url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &payload.refresh_token),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ])
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !resp.status().is_success() {
        // Keycloak отвечает 400 с error=invalid_grant, когда refresh-токен
        // недействителен или уже использован — для клиента это «войдите заново».
        return Err(StatusCode::UNAUTHORIZED.into());
    }
    let json: serde_json::Value = resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let token = json
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or(StatusCode::BAD_GATEWAY)?;
    // Keycloak ротирует refresh-токен — возвращаем новый, чтобы клиент сохранил.
    let new_refresh = json
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .unwrap_or(&payload.refresh_token);

    let body = Json(serde_json::json!({
        "access_token": token,
        "refresh_token": new_refresh,
        "expires_in": json.get("expires_in"),
    }))
    .into_response();
    let mut resp = body;
    let cookie = format!("aga_token={token}; Path=/; HttpOnly; SameSite=Lax");
    resp.headers_mut()
        .insert(axum::http::header::SET_COOKIE, cookie.parse()?);
    Ok(resp)
}
