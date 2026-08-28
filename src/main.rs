mod agent;
mod auth;
mod chat;
mod cluster;
mod config;
mod llm;
mod reactive;
mod server;
mod trace;
mod workstation;

use std::env;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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

    tracing::info!("Загрузка конфигурации из {}", config_path);
    let config = config::Config::load(&config_path)?;

    tracing::info!("Инициализация базы данных: {}", db_path);
    let trace_store = trace::TraceStore::new(&db_path).await?;
    tracing::info!("TraceStore ok");

    tracing::info!("Инициализация модели чата: {}", db_path);
    let chat_store = chat::ChatStore::new(&db_path).await?;
    tracing::info!("ChatStore ok");

    // Учётка агента для каждой настроенной роли.
    for role in config.roles.keys() {
        if let Ok(id) = chat_store.ensure_agent_user(role).await {
            tracing::debug!("agent user for role {role}: {id}");
        }
    }

    tracing::info!("Подключение к LLM API: {}", llm_api_url);
    let llm_client = llm::LlmClient::new(&llm_api_url, llm_api_key);

    let cluster = cluster::Cluster::from_env();
    tracing::info!(
        "Kubernetes: kubectl={} namespace={} template={} image={}",
        cluster.kubectl,
        cluster.namespace,
        cluster.template,
        cluster.image
    );

    let reactive = reactive::ReactiveRunner::new(
        config.clone(),
        llm_client.clone(),
        trace_store.clone(),
        chat_store.clone(),
        cluster.clone(),
    );

    // Создаём состояние приложения
    let state = server::AppState {
        config,
        trace_store,
        llm_client,
        chat_store,
        reactive,
        cluster,
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
