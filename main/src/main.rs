mod agent;
mod auth;
mod centrifuge;
mod chat;
mod cluster;
mod config;
mod git_changes;
mod llm;
mod project_files;
mod runtime;
mod scope;
mod seed;
mod server;
mod ssh_key;
mod trace;
mod workstation;
mod ws_ops;

use std::env;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Скачать JWKS-документ по URL (тело — JSON).
async fn fetch_jwks(url: &str) -> Result<String, reqwest::Error> {
    reqwest::get(url).await?.text().await
}

/// Как часто перечитывать JWKS (сек). Keycloak пересоздаёт ключ подписи при
/// рестарте/переимпорте realm — ядро живёт дольше и без перечитывания
/// отвергло бы все свежие токены (SSO-петля входа).
const JWKS_REFRESH_SECS: u64 = 300;

/// Фоновая задача: периодически тянет JWKS заново и подменяет верификатор.
/// Сбой не фатален — оставляем прежний ключ и логируем.
async fn refresh_jwks_loop(url: String, verifier: Arc<RwLock<Option<auth::JwtVerifier>>>) {
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(JWKS_REFRESH_SECS));
    // Первый тик срабатывает сразу — пропускаем, JWKS уже загружен при старте.
    ticker.tick().await;
    loop {
        ticker.tick().await;
        match fetch_jwks(&url).await {
            Ok(jwks) => match auth::JwtVerifier::from_jwks_json(&jwks) {
                Ok(fresh) => {
                    *verifier.write().await = Some(fresh);
                    tracing::info!("JWKS обновлён из {url}");
                }
                Err(e) => tracing::warn!("Не удалось разобрать обновлённый JWKS из {url}: {e}"),
            },
            Err(e) => tracing::warn!("Не удалось обновить JWKS из {url}: {e}"),
        }
    }
}

/// Режимы одного бинаря: HTTP-сервер ядра (`aga`), восстановление тестового
/// набора (`aga seed`) и агент-рантайм (`aga agent`) — отдельный процесс,
/// подписанный на Centrifugo.
#[derive(Debug, PartialEq, Eq)]
pub enum RunMode {
    Server,
    Seed,
    Agent,
}

pub fn mode_from_args(args: &[String]) -> RunMode {
    match args.get(1).map(String::as_str) {
        Some("seed") => RunMode::Seed,
        Some("agent") => RunMode::Agent,
        _ => RunMode::Server,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Инициализация логирования
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Путь к БД читаем до конфига: seed и agent-режим живут по своим путям.
    let db_path = env::var("AGA_DB_PATH").unwrap_or_else(|_| "./data/trace.db".to_string());

    let args: Vec<String> = std::env::args().collect();
    match mode_from_args(&args) {
        // `aga seed` — восстановить тестовый набор в БД (см. seed.rs).
        RunMode::Seed => return seed::seed(&db_path).await,
        // `aga agent` — агент-рантайм: отдельный процесс без HTTP-сервера
        // (см. runtime.rs).
        RunMode::Agent => return runtime::agent_main(&db_path).await,
        RunMode::Server => {}
    }

    // Загружаем конфигурацию из переменных окружения
    let config_path =
        env::var("AGA_CONFIG_PATH").unwrap_or_else(|_| "./config/roles.yaml".to_string());

    tracing::info!("Загрузка конфигурации из {}", config_path);
    let config = config::Config::load(&config_path)?;

    // Centrifugo (реальное время для чата). Не задан — клиент-заглушка: чат без
    // автообновления, /connection-jwt/ отдаёт 404.
    let centrifuge = match config.centrifuge.as_ref() {
        Some(cfg) => {
            tracing::info!(
                "Centrifugo: api_url={} channel={}",
                cfg.api_url,
                cfg.channel
            );
            centrifuge::CentrifugeClient::from_config(cfg)
        }
        None => {
            tracing::warn!("Centrifugo не настроен — чат без автообновления");
            centrifuge::CentrifugeClient::disabled()
        }
    };

    tracing::info!("Инициализация базы данных: {}", db_path);
    let trace_store = trace::TraceStore::new(&db_path).await?;
    tracing::info!("TraceStore ok");

    tracing::info!("Инициализация модели чата: {}", db_path);
    let chat_store = chat::ChatStore::new(&db_path).await?;
    tracing::info!("ChatStore ok");

    // Верификатор JWT против JWKS. Включён только когда SSO включён и задан
    // jwks_url; иначе запросы работают под аноним-суперпользователем. Обёрнут
    // в RwLock — фоновая задача refresh_jwks_loop периодически подменяет его
    // свежим JWKS (Keycloak меняет ключ подписи при переимпорте realm).
    let (sso_verifier, sso_jwks_url) = match config.sso.as_ref().filter(|s| s.enabled) {
        Some(sso) => match sso.jwks_url.as_deref() {
            Some(url) => {
                // Keycloak (стенд и dev-стенд) стартует дольше ядра — тянем JWKS
                // с ретраями, чтобы ядро не поднялось без SSO и не открыло
                // анонимный доступ. Без JWKS при включённом SSO запуск запрещён.
                tracing::info!("Загрузка JWKS из {}", url);
                const ATTEMPTS: u32 = 30;
                let mut fetched: Option<String> = None;
                for attempt in 1..=ATTEMPTS {
                    match fetch_jwks(url).await {
                        Ok(jwks) => {
                            fetched = Some(jwks);
                            break;
                        }
                        Err(e) => {
                            if attempt == ATTEMPTS {
                                tracing::error!("Не удалось загрузить JWKS из {url}: {e}");
                            } else {
                                tracing::warn!(
                                    "JWKS из {url} пока недоступен: {e}; попытка {attempt}/{ATTEMPTS}"
                                );
                                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            }
                        }
                    }
                }
                let jwks = fetched.ok_or_else(|| -> Box<dyn std::error::Error> {
                    format!("SSO включён, но JWKS из {url} недоступен — запуск без SSO недопустим")
                        .into()
                })?;
                let verifier = auth::JwtVerifier::from_jwks_json(&jwks)
                    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                (Some(verifier), Some(url.to_string()))
            }
            None => {
                tracing::warn!("SSO включён, но jwks_url не задан — работаем без SSO");
                (None, None)
            }
        },
        None => (None, None),
    };
    let sso_verifier = Arc::new(RwLock::new(sso_verifier));
    if let Some(url) = sso_jwks_url {
        tokio::spawn(refresh_jwks_loop(url, sso_verifier.clone()));
    }

    // Origin веб-клиента (front/): CORS-источник и адрес возврата токена после SSO.
    let front_url =
        env::var("AGA_FRONT_URL").unwrap_or_else(|_| "http://dev.localhost".to_string());
    tracing::info!("Frontend origin: {}", front_url);

    let cluster = cluster::Cluster::from_env();
    tracing::info!(
        "Workstations: backend={:?} kubectl={} namespace={} template={} image={}",
        cluster.backend,
        cluster.kubectl,
        cluster.namespace,
        cluster.template,
        cluster.image
    );

    // Создаём состояние приложения
    let state = server::AppState {
        config,
        trace_store,
        chat_store,
        cluster,
        centrifuge,
        sso_verifier,
        front_url,
    };

    // Создаём роутер
    let app = server::create_router(state);

    // Запускаем сервер
    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{port}");
    tracing::info!("Запуск HTTP сервера на {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(rest: &[&str]) -> Vec<String> {
        std::iter::once("aga".to_string())
            .chain(rest.iter().map(|s| s.to_string()))
            .collect()
    }

    #[test]
    fn agent_arg_selects_runtime_not_http_server() {
        assert_eq!(mode_from_args(&args(&["agent"])), RunMode::Agent);
    }

    #[test]
    fn no_arg_selects_http_server() {
        assert_eq!(mode_from_args(&args(&[])), RunMode::Server);
    }

    #[test]
    fn seed_arg_selects_seed() {
        assert_eq!(mode_from_args(&args(&["seed"])), RunMode::Seed);
    }
}
