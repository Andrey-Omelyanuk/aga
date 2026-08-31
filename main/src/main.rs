mod agent;
mod auth;
mod chat;
mod cluster;
mod config;
mod llm;
mod project_files;
mod reactive;
mod scope;
mod seed;
mod server;
mod trace;
mod workstation;
mod ws_ops;

use std::env;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Скачать JWKS-документ по URL (тело — JSON).
async fn fetch_jwks(url: &str) -> Result<String, reqwest::Error> {
    reqwest::get(url).await?.text().await
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

    // Путь к БД читаем до конфига: seed работает без roles.yaml.
    let db_path = env::var("AGA_DB_PATH").unwrap_or_else(|_| "./data/trace.db".to_string());

    // `aga seed` — восстановить тестовый набор в БД (см. seed.rs).
    if std::env::args().nth(1).as_deref() == Some("seed") {
        return seed::seed(&db_path).await;
    }

    // Загружаем конфигурацию из переменных окружения
    let config_path =
        env::var("AGA_CONFIG_PATH").unwrap_or_else(|_| "./config/roles.yaml".to_string());
    let llm_api_url =
        env::var("LLM_API_URL").unwrap_or_else(|_| "http://localhost:11434/v1".to_string());
    let llm_api_key = env::var("LLM_API_KEY").ok();
    let llm_default_model = env::var("LLM_MODEL").unwrap_or_else(|_| "qwen3.5:9b".to_string());

    tracing::info!("Загрузка конфигурации из {}", config_path);
    let config = config::Config::load(&config_path)?;

    tracing::info!("Инициализация базы данных: {}", db_path);
    let trace_store = trace::TraceStore::new(&db_path).await?;
    tracing::info!("TraceStore ok");

    tracing::info!("Инициализация модели чата: {}", db_path);
    let chat_store = chat::ChatStore::new(&db_path).await?;
    tracing::info!("ChatStore ok");

    // Верификатор JWT против JWKS. Включён только когда SSO включён и задан
    // jwks_url; иначе запросы работают под аноним-суперпользователем.
    let sso_verifier = match config.sso.as_ref().filter(|s| s.enabled) {
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
                Some(
                    auth::JwtVerifier::from_jwks_json(&jwks)
                        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?,
                )
            }
            None => {
                tracing::warn!("SSO включён, но jwks_url не задан — работаем без SSO");
                None
            }
        },
        None => None,
    };

    // Origin веб-клиента (front/): CORS-источник и адрес возврата токена после SSO.
    let front_url =
        env::var("AGA_FRONT_URL").unwrap_or_else(|_| "http://dev.localhost".to_string());
    tracing::info!("Frontend origin: {}", front_url);

    tracing::info!("Подключение к LLM API: {}", llm_api_url);
    let llm_client = llm::LlmClient::new(&llm_api_url, llm_api_key, &llm_default_model);

    let cluster = cluster::Cluster::from_env();
    tracing::info!(
        "Workstations: backend={:?} kubectl={} namespace={} template={} image={}",
        cluster.backend,
        cluster.kubectl,
        cluster.namespace,
        cluster.template,
        cluster.image
    );

    let reactive = reactive::ReactiveRunner::new(
        llm_client.clone(),
        trace_store.clone(),
        chat_store.clone(),
        cluster.clone(),
    );

    // Создаём состояние приложения
    let state = server::AppState {
        config,
        trace_store,
        chat_store,
        reactive,
        cluster,
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
