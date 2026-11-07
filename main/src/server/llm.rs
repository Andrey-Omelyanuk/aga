//! Подключения к LLM (страница «LLM») и выбор дефолтной LLM.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};

use super::{current_user, merge_secret, new_secret, require_admin, AppError, AppState};

/// Подключение к LLM наружу: ключ доступа не отдаётся (только признак
/// `has_api_key`) — список подключений видят все участники.
#[derive(Serialize)]
pub struct LlmConnectionView {
    pub id: i64,
    pub name: String,
    pub api_url: String,
    pub has_api_key: bool,
    pub model_name: String,
    pub is_default: bool,
    pub native_tools: bool,
    pub max_tokens: Option<i64>,
    pub temperature: Option<f64>,
}

impl From<crate::trace::LlmConnection> for LlmConnectionView {
    fn from(c: crate::trace::LlmConnection) -> Self {
        Self {
            id: c.id,
            name: c.name,
            api_url: c.api_url,
            has_api_key: c.api_key.as_deref().is_some_and(|k| !k.is_empty()),
            model_name: c.model_name,
            is_default: c.is_default,
            native_tools: c.native_tools,
            max_tokens: c.max_tokens,
            temperature: c.temperature,
        }
    }
}

#[derive(Deserialize)]
pub struct LlmConnectionRequest {
    pub name: String,
    pub api_url: String,
    pub api_key: Option<String>,
    pub model_name: String,
    /// Нативный function calling; не задан — true (текстовый режим — запасной).
    #[serde(default = "crate::trace::default_native_tools")]
    pub native_tools: bool,
    /// Предел токенов ответа (1..=1 000 000); не задан — дефолт клиента.
    #[serde(default)]
    pub max_tokens: Option<i64>,
    /// Температура (0..=2); не задана — дефолт клиента (0.7).
    #[serde(default)]
    pub temperature: Option<f64>,
}

impl LlmConnectionRequest {
    /// Параметры генерации в допустимых пределах — иначе 400.
    fn check(&self) -> Result<(), StatusCode> {
        let tokens_ok = self.max_tokens.is_none_or(|t| (1..=1_000_000).contains(&t));
        let temp_ok = self
            .temperature
            .is_none_or(|t| t.is_finite() && (0.0..=2.0).contains(&t));
        if tokens_ok && temp_ok {
            Ok(())
        } else {
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

/// Выбор дефолтной LLM на странице «LLM»: id подключения или null (снять выбор).
#[derive(Deserialize)]
pub struct SetLlmDefaultRequest {
    pub llm_id: Option<i64>,
}

// === API для управления подключениями к LLM ===

pub(super) async fn list_llm_connections(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<LlmConnectionView>>, AppError> {
    current_user(&state, &headers).await?;
    let list = state.trace_store.list_llm_connections().await?;
    Ok(Json(
        list.into_iter().map(LlmConnectionView::from).collect(),
    ))
}

pub(super) async fn create_llm_connection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LlmConnectionRequest>,
) -> Result<Json<LlmConnectionView>, AppError> {
    require_admin(&state, &headers).await?;
    payload.check()?;
    let spec = crate::trace::LlmConnectionSpec {
        name: payload.name,
        api_url: payload.api_url,
        api_key: new_secret(payload.api_key),
        model_name: payload.model_name,
        native_tools: payload.native_tools,
        max_tokens: payload.max_tokens,
        temperature: payload.temperature,
    };
    let id = state.trace_store.create_llm_connection(&spec).await?;
    let connection = state
        .trace_store
        .get_llm_connection(id)
        .await?
        .ok_or_else(|| AppError::internal(format!("created llm connection {id} not found")))?;
    Ok(Json(connection.into()))
}

pub(super) async fn get_llm_connection(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<LlmConnectionView>, AppError> {
    current_user(&state, &headers).await?;
    let connection = state
        .trace_store
        .get_llm_connection(id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(connection.into()))
}

pub(super) async fn delete_llm_connection(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    require_admin(&state, &headers).await?;
    if state.trace_store.delete_llm_connection(id).await? {
        Ok(StatusCode::OK)
    } else {
        Err(StatusCode::NOT_FOUND.into())
    }
}

/// Изменить подключение (название, url API, ключ, модель). Возвращает
/// обновлённое подключение; используется агентами набора сразу после правки.
pub(super) async fn update_llm_connection(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LlmConnectionRequest>,
) -> Result<Json<LlmConnectionView>, AppError> {
    require_admin(&state, &headers).await?;
    payload.check()?;
    let old = state
        .trace_store
        .get_llm_connection(id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    let spec = crate::trace::LlmConnectionSpec {
        name: payload.name,
        api_url: payload.api_url,
        api_key: merge_secret(payload.api_key, old.api_key),
        model_name: payload.model_name,
        native_tools: payload.native_tools,
        max_tokens: payload.max_tokens,
        temperature: payload.temperature,
    };
    if !state.trace_store.update_llm_connection(id, &spec).await? {
        return Err(StatusCode::NOT_FOUND.into());
    }
    let connection = state
        .trace_store
        .get_llm_connection(id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(connection.into()))
}

/// Выбрать дефолтную LLM: `llm_id` — одно из подключений (дефолт снимается с
/// остальных — одна дефолтная LLM), `null` — снять выбор. Возвращает дефолтное
/// подключение или null.
pub(super) async fn set_llm_default(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SetLlmDefaultRequest>,
) -> Result<Json<Option<LlmConnectionView>>, AppError> {
    require_admin(&state, &headers).await?;
    match payload.llm_id {
        Some(id) => {
            if !state.trace_store.set_default_llm(id).await? {
                return Err(StatusCode::NOT_FOUND.into());
            }
            let connection = state
                .trace_store
                .get_llm_connection(id)
                .await?
                .ok_or(StatusCode::NOT_FOUND)?;
            Ok(Json(Some(connection.into())))
        }
        None => {
            state.trace_store.clear_default_llm().await?;
            Ok(Json(None))
        }
    }
}
