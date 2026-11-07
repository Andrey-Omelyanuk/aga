//! Вход через SSO: login (state + PKCE), callback, logout, refresh.

use super::*;
use crate::server::sso::{cookie_secure_attr, pkce_challenge};

#[tokio::test]
async fn login_redirects_to_keycloak_when_configured() {
    let (state, file) = test_state(true).await;
    // Проверяем /auth/login: без SSO-конфигурации эндпоинтов — 404.
    let _headers = HeaderMap::new();
    let router = create_router(state.clone());
    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/auth/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    cleanup(&file).await;
}

#[tokio::test]
async fn login_redirects_with_absolute_redirect_uri_from_host() {
    let (mut state, file) = test_state(false).await;
    state.config.sso = Some(crate::config::SsoConfig {
        enabled: true,
        jwks_url: None,
        authorize_url: Some("http://auth.localhost/realms/aga/protocol/openid-connect/auth".into()),
        token_url: None,
        end_session_url: None,
        client_id: Some("aga".into()),
        client_secret: None,
    });
    let router = create_router(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/auth/login")
                .header("Host", "dev.localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    let location = resp
        .headers()
        .get(axum::http::header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    let url = url::Url::parse(location).unwrap();
    let redirect_uri = url
        .query_pairs()
        .find(|(k, _)| k == "redirect_uri")
        .map(|(_, v)| v.into_owned())
        .unwrap();
    // Абсолютный redirect_uri на хосте SPA, иначе Keycloak после входа
    // вернёт браузер на свой хост (auth.localhost) и будет 404.
    assert_eq!(redirect_uri, "http://dev.localhost/auth/callback");
    cleanup(&file).await;
}

fn sso_for_login() -> crate::config::SsoConfig {
    crate::config::SsoConfig {
        enabled: true,
        jwks_url: None,
        authorize_url: Some("http://auth.localhost/realms/aga/protocol/openid-connect/auth".into()),
        token_url: None,
        end_session_url: None,
        client_id: Some("aga".into()),
        client_secret: Some("aga-secret".into()),
    }
}

/// Вход уносит в Keycloak случайный state и PKCE-challenge (S256); state и
/// верификатор остаются в HttpOnly-cookie только для пути колбэка.
#[tokio::test]
async fn login_sends_state_and_pkce_challenge_and_keeps_them_in_cookie() {
    let (mut state, file) = test_state(false).await;
    state.config.sso = Some(sso_for_login());
    let resp = create_router(state)
        .oneshot(
            Request::builder()
                .uri("/auth/login")
                .header("Host", "api.localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    let location = resp.headers()[axum::http::header::LOCATION]
        .to_str()
        .unwrap();
    let pairs: std::collections::HashMap<String, String> = url::Url::parse(location)
        .unwrap()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let cookie = resp.headers()[axum::http::header::SET_COOKIE]
        .to_str()
        .unwrap();
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.contains("Path=/auth/callback"), "{cookie}");
    assert!(!cookie.contains("Secure"), "http-стенд: {cookie}");
    let value = cookie
        .strip_prefix("aga_oauth=")
        .and_then(|v| v.split(';').next())
        .unwrap();
    let (cookie_state, verifier) = value.split_once('.').unwrap();
    assert_eq!(pairs["state"], cookie_state);
    assert_eq!(pairs["code_challenge_method"], "S256");
    assert_eq!(pairs["code_challenge"], pkce_challenge(verifier));
    assert!(verifier.len() >= 43 && verifier.len() <= 128);
    // Каждый вход — новый state.
    let (mut state2, file2) = test_state(false).await;
    state2.config.sso = Some(sso_for_login());
    let resp2 = create_router(state2)
        .oneshot(
            Request::builder()
                .uri("/auth/login")
                .header("Host", "api.localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cookie2 = resp2.headers()[axum::http::header::SET_COOKIE]
        .to_str()
        .unwrap();
    assert_ne!(cookie, cookie2);
    cleanup(&file).await;
    cleanup(&file2).await;
}

#[test]
fn pkce_challenge_matches_rfc7636_example() {
    // RFC 7636, приложение B.
    assert_eq!(
        pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn cookies_are_secure_behind_https() {
    assert_eq!(
        cookie_secure_attr("https://api.example.com/auth/callback"),
        "; Secure"
    );
    assert_eq!(cookie_secure_attr("http://api.localhost/auth/callback"), "");
}

/// Мок token-эндпоинта, который запоминает тело запроса обмена кода.
async fn recording_token_server() -> (
    String,
    std::sync::Arc<std::sync::Mutex<String>>,
    tokio::task::JoinHandle<()>,
) {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let rec = seen.clone();
    let app = axum::Router::new().route(
        "/token",
        axum::routing::post(move |body: String| {
            let rec = rec.clone();
            async move {
                *rec.lock().unwrap() = body;
                axum::Json(serde_json::json!({
                    "access_token": "fresh.access.token",
                    "refresh_token": "fresh.refresh.token",
                }))
            }
        }),
    );
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}/token"), seen, handle)
}

async fn callback(state: AppState, query: &str, cookie: Option<&str>) -> Response {
    let mut req = Request::builder()
        .uri(format!("/auth/callback?{query}"))
        .header("Host", "api.localhost");
    if let Some(c) = cookie {
        req = req.header("cookie", c);
    }
    create_router(state)
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// Колбэк без cookie флоу или с чужим state — 400: code, подсунутый
/// чужой ссылкой (login-CSRF), не обменивается.
#[tokio::test]
async fn callback_rejects_missing_or_foreign_state() {
    let (mut state, file) = test_state(false).await;
    let (token_url, seen, _server) = recording_token_server().await;
    let mut sso = sso_for_login();
    sso.token_url = Some(token_url);
    state.config.sso = Some(sso);
    let resp = callback(state.clone(), "code=c1&state=s1", None).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let resp = callback(state.clone(), "code=c1&state=evil", Some("aga_oauth=s1.v1")).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let resp = callback(state.clone(), "code=c1", Some("aga_oauth=s1.v1")).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    // До Keycloak такие запросы не доходят.
    assert!(seen.lock().unwrap().is_empty());
    cleanup(&file).await;
}

/// Совпавший state: код меняется на токен вместе с PKCE-верификатором,
/// токены — фрагментом на фронт, cookie флоу стирается.
#[tokio::test]
async fn callback_with_matching_state_exchanges_code_with_verifier() {
    let (mut state, file) = test_state(false).await;
    let (token_url, seen, _server) = recording_token_server().await;
    let mut sso = sso_for_login();
    sso.token_url = Some(token_url);
    state.config.sso = Some(sso);
    let resp = callback(
        state,
        "code=c1&state=s1",
        Some("other=1; aga_oauth=s1.verifier-123"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    let body = seen.lock().unwrap().clone();
    assert!(body.contains("code=c1"), "{body}");
    assert!(body.contains("code_verifier=verifier-123"), "{body}");
    assert!(
        body.contains("redirect_uri=http%3A%2F%2Fapi.localhost%2Fauth%2Fcallback"),
        "{body}"
    );
    let location = resp.headers()[axum::http::header::LOCATION]
        .to_str()
        .unwrap();
    assert_eq!(
        location,
        "http://dev.localhost#token=fresh.access.token&refresh=fresh.refresh.token"
    );
    let cookies: Vec<&str> = resp
        .headers()
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect();
    assert!(cookies
        .iter()
        .any(|c| c.starts_with("aga_token=fresh.access.token")));
    assert!(cookies
        .iter()
        .any(|c| c.starts_with("aga_oauth=;") && c.contains("Max-Age=0")));
    cleanup(&file).await;
}

#[tokio::test]
async fn callback_with_error_returns_bad_request() {
    let (mut state, file) = test_state(false).await;
    state.config.sso = Some(crate::config::SsoConfig {
        enabled: true,
        jwks_url: None,
        authorize_url: None,
        token_url: None,
        end_session_url: None,
        client_id: Some("aga".into()),
        client_secret: None,
    });
    let router = create_router(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/auth/callback?error=login_required")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    cleanup(&file).await;
}

#[tokio::test]
async fn logout_clears_cookie_and_redirects_to_keycloak_end_session() {
    let (mut state, file) = test_state(false).await;
    state.config.sso = Some(crate::config::SsoConfig {
        enabled: true,
        jwks_url: None,
        authorize_url: None,
        token_url: None,
        end_session_url: Some(
            "http://auth.localhost/realms/aga/protocol/openid-connect/logout".into(),
        ),
        client_id: Some("aga".into()),
        client_secret: None,
    });
    let router = create_router(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/auth/logout")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    // Cookie aga_token сброшен (HttpOnly-куку может стереть только сервер).
    let set_cookie = resp
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(set_cookie.starts_with("aga_token="));
    assert!(set_cookie.contains("Max-Age=0"));
    // Редирект на end-session Keycloak с возвратом на фронт.
    let location = resp
        .headers()
        .get(axum::http::header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    let url = url::Url::parse(location).unwrap();
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert_eq!(
        url.host_str(),
        Some("auth.localhost"),
        "end_session: {location}"
    );
    assert!(pairs.contains(&("client_id".into(), "aga".into())));
    assert!(pairs.contains(&(
        "post_logout_redirect_uri".into(),
        "http://dev.localhost".into()
    )));
    cleanup(&file).await;
}

/// Мок token-эндпоинта Keycloak на случайном порту: успешный обмен
/// (свежие токены) или 400 invalid_grant.
async fn mock_token_server(success: bool) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = axum::Router::new().route(
        "/token",
        axum::routing::post(move |_: axum::extract::Request| async move {
            if success {
                (
                    axum::http::StatusCode::OK,
                    axum::Json(serde_json::json!({
                        "access_token": "fresh.access.token",
                        "refresh_token": "fresh.refresh.token",
                        "expires_in": 300,
                    })),
                )
            } else {
                (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({ "error": "invalid_grant" })),
                )
            }
        }),
    );
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}/token"), handle)
}

fn sso_with_token_url(token_url: String) -> crate::config::SsoConfig {
    crate::config::SsoConfig {
        enabled: true,
        jwks_url: None,
        authorize_url: None,
        token_url: Some(token_url),
        end_session_url: None,
        client_id: Some("aga".into()),
        client_secret: Some("aga-secret".into()),
    }
}

#[tokio::test]
async fn refresh_exchanges_token_and_sets_cookie() {
    let (mut state, file) = test_state(false).await;
    let (token_url, _server) = mock_token_server(true).await;
    state.config.sso = Some(sso_with_token_url(token_url));
    let router = create_router(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/refresh")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"refresh_token":"stale.refresh.token"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let set_cookie = resp
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(set_cookie.starts_with("aga_token="));
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["access_token"], "fresh.access.token");
    assert_eq!(json["refresh_token"], "fresh.refresh.token");
    cleanup(&file).await;
}

#[tokio::test]
async fn refresh_rejects_invalid_token() {
    let (mut state, file) = test_state(false).await;
    let (token_url, _server) = mock_token_server(false).await;
    state.config.sso = Some(sso_with_token_url(token_url));
    let router = create_router(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/refresh")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"refresh_token":"stale.refresh.token"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    cleanup(&file).await;
}

#[tokio::test]
async fn refresh_requires_sso_config() {
    let (state, file) = test_state(false).await;
    let router = create_router(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/refresh")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"refresh_token":"x"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    cleanup(&file).await;
}
