mod agent;
mod auth;
mod chat;
mod cluster;
mod config;
mod llm;
mod project_files;
mod reactive;
mod scope;
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

    // Загружаем конфигурацию из переменных окружения
    let config_path =
        env::var("AGA_CONFIG_PATH").unwrap_or_else(|_| "./config/roles.yaml".to_string());
    let db_path = env::var("AGA_DB_PATH").unwrap_or_else(|_| "./data/trace.db".to_string());
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
                tracing::info!("Загрузка JWKS из {}", url);
                match fetch_jwks(url).await {
                    Ok(jwks) => Some(
                        auth::JwtVerifier::from_jwks_json(&jwks)
                            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?,
                    ),
                    Err(e) => {
                        tracing::error!("Не удалось загрузить JWKS из {url}: {e}");
                        None
                    }
                }
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
