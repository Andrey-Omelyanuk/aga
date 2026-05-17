mod config;
mod trace;
mod llm;
mod agent;
mod server;

use std::env;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Инициализация логирования
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Загружаем конфигурацию из переменных окружения
    let config_path = env::var("AGA_CONFIG_PATH").unwrap_or_else(|_| "./config/roles.yaml".to_string());
    let db_path = env::var("AGA_DB_PATH").unwrap_or_else(|_| "./data/trace.db".to_string());
    let llm_api_url = env::var("LLM_API_URL").unwrap_or_else(|_| "http://localhost:11434/v1".to_string());
    let llm_api_key = env::var("LLM_API_KEY").ok();

    tracing::info!("Загрузка конфигурации из {}", config_path);
    let config = config::Config::load(&config_path)?;

    tracing::info!("Инициализация базы данных: {}", db_path);
    let trace_store = trace::TraceStore::new(&db_path).await?;

    tracing::info!("Подключение к LLM API: {}", llm_api_url);
    let llm_client = llm::LlmClient::new(&llm_api_url, llm_api_key);

    // Создаём состояние приложения
    let state = server::AppState {
        config,
        trace_store,
        llm_client,
    };

    // Создаём роутер
    let app = server::create_router(state);

    // Запускаем сервер
    let addr = "0.0.0.0:8080";
    tracing::info!("Запуск HTTP сервера на {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
