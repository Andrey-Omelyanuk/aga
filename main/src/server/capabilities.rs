//! Каталог способностей: скиллы и сокращения (`capabilities` с `kind`), у
//! записи одно текущее содержимое и история изменений (кто, когда и что сделал).

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use super::{current_actor, current_user, AppError, AppState};

#[derive(Deserialize)]
pub struct CreateCapabilityRequest {
    pub name: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct UpdateCapabilityRequest {
    pub name: Option<String>,
    pub content: Option<String>,
}

/// Фильтр «Удалённые»: `?deleted=1` — только удалённые записи каталога.
#[derive(Deserialize)]
pub struct DeletedFilter {
    pub deleted: Option<i64>,
}

/// Общий список записей каталога вида: активные или «Удалённые» (`?deleted=1`).
async fn list_capabilities(
    kind: crate::trace::CapabilityKind,
    state: &AppState,
    headers: &HeaderMap,
    deleted: Option<i64>,
) -> Result<Json<Vec<crate::trace::CapabilityItem>>, AppError> {
    current_user(state, headers).await?;
    let items = match deleted {
        Some(1) => state.trace_store.list_deleted_capabilities(kind).await?,
        _ => state.trace_store.list_capabilities(kind, false).await?,
    };
    Ok(Json(items))
}

pub(super) async fn list_skills(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(filter): Query<DeletedFilter>,
) -> Result<Json<Vec<crate::trace::CapabilityItem>>, AppError> {
    list_capabilities(
        crate::trace::CapabilityKind::Skill,
        &state,
        &headers,
        filter.deleted,
    )
    .await
}

pub(super) async fn create_skill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateCapabilityRequest>,
) -> Result<Json<crate::trace::CapabilityItem>, AppError> {
    create_capability(
        crate::trace::CapabilityKind::Skill,
        &state,
        &headers,
        payload,
    )
    .await
}

async fn create_capability(
    kind: crate::trace::CapabilityKind,
    state: &AppState,
    headers: &HeaderMap,
    payload: CreateCapabilityRequest,
) -> Result<Json<crate::trace::CapabilityItem>, AppError> {
    let (actor, actor_name) = current_actor(state, headers).await?;
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err(StatusCode::BAD_REQUEST.into());
    }
    // Имя в пределах вида уникально (в т.ч. среди удалённых) — дубль это 409.
    if state
        .trace_store
        .capability_name_taken(kind, &name, 0)
        .await?
    {
        return Err(StatusCode::CONFLICT.into());
    }
    let id = state
        .trace_store
        .create_capability(kind, &name, &payload.content, actor, &actor_name)
        .await?;
    let item = state
        .trace_store
        .get_capability(id)
        .await?
        .ok_or_else(|| AppError::internal(format!("created capability {id} not found")))?;
    Ok(Json(item))
}

pub(super) async fn get_skill(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::trace::CapabilityItem>, AppError> {
    current_user(&state, &headers).await?;
    let item = state
        .trace_store
        .get_capability(id)
        .await?
        .filter(|c| c.kind == crate::trace::CapabilityKind::Skill)
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(item))
}

/// Правка записи: имя и/или содержимое; каждое изменение пишет запись истории.
pub(super) async fn update_skill(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdateCapabilityRequest>,
) -> Result<Json<crate::trace::CapabilityItem>, AppError> {
    update_capability(
        crate::trace::CapabilityKind::Skill,
        id,
        &state,
        &headers,
        payload,
    )
    .await
}

async fn update_capability(
    kind: crate::trace::CapabilityKind,
    id: i64,
    state: &AppState,
    headers: &HeaderMap,
    payload: UpdateCapabilityRequest,
) -> Result<Json<crate::trace::CapabilityItem>, AppError> {
    let (actor, actor_name) = current_actor(state, headers).await?;
    let current = state
        .trace_store
        .get_capability(id)
        .await?
        .filter(|c| c.kind == kind)
        .ok_or(StatusCode::NOT_FOUND)?;
    // Имя меняем только если оно реально изменилось: правка содержимого с тем же
    // именем не должна писать в историю «переименовал». Совпадение с уже занятым
    // именем (активным или удалённым) — 409, а не 500.
    if let Some(name) = payload
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
    {
        if name != current.name {
            if state
                .trace_store
                .capability_name_taken(kind, &name, id)
                .await?
            {
                return Err(StatusCode::CONFLICT.into());
            }
            if !state
                .trace_store
                .rename_capability(id, &name, actor, &actor_name)
                .await?
            {
                return Err(StatusCode::NOT_FOUND.into());
            }
        }
    }
    // Содержимое — тоже только при реальном изменении: веб-клиент шлёт его при
    // каждом сохранении, и переименование писало бы пустое «изменил содержимое».
    if let Some(content) = payload.content.filter(|c| *c != current.content) {
        if !state
            .trace_store
            .update_capability_content(id, &content, actor, &actor_name)
            .await?
        {
            return Err(StatusCode::NOT_FOUND.into());
        }
    }
    let item = state
        .trace_store
        .get_capability(id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(item))
}

pub(super) async fn delete_skill(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    delete_capability(crate::trace::CapabilityKind::Skill, id, &state, &headers).await
}

async fn delete_capability(
    kind: crate::trace::CapabilityKind,
    id: i64,
    state: &AppState,
    headers: &HeaderMap,
) -> Result<StatusCode, AppError> {
    let (actor, actor_name) = current_actor(state, headers).await?;
    let exists = state
        .trace_store
        .get_capability(id)
        .await?
        .filter(|c| c.kind == kind)
        .is_some();
    if !exists {
        return Err(StatusCode::NOT_FOUND.into());
    }
    if state
        .trace_store
        .delete_capability(id, actor, &actor_name)
        .await?
    {
        Ok(StatusCode::OK)
    } else {
        Err(StatusCode::NOT_FOUND.into())
    }
}

/// История изменений записи каталога: кто, когда и что сделал, по порядку.
pub(super) async fn capability_history(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::trace::CapabilityHistoryEntry>>, AppError> {
    current_user(&state, &headers).await?;
    if state.trace_store.get_capability(id).await?.is_none() {
        return Err(StatusCode::NOT_FOUND.into());
    }
    Ok(Json(state.trace_store.capability_history(id).await?))
}

// === Сокращения (shortcuts) ===
// Живут в общем каталоге (kind='shortcut') с той же историей, что скиллы.
// Отличие — имя не содержит пробелов: вызывается оно словом `/имя`,
// которое браузер не разобьёт на части.

/// Имя сокращения: непустое и без пробелов.
fn shortcut_name_valid(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(|c| c.is_whitespace())
}

pub(super) async fn list_shortcuts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(filter): Query<DeletedFilter>,
) -> Result<Json<Vec<crate::trace::CapabilityItem>>, AppError> {
    list_capabilities(
        crate::trace::CapabilityKind::Shortcut,
        &state,
        &headers,
        filter.deleted,
    )
    .await
}

pub(super) async fn create_shortcut(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateCapabilityRequest>,
) -> Result<Json<crate::trace::CapabilityItem>, AppError> {
    let name = payload.name.trim().to_string();
    if !shortcut_name_valid(&name) {
        return Err(StatusCode::BAD_REQUEST.into());
    }
    create_capability(
        crate::trace::CapabilityKind::Shortcut,
        &state,
        &headers,
        CreateCapabilityRequest {
            name,
            content: payload.content,
        },
    )
    .await
}

pub(super) async fn get_shortcut(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::trace::CapabilityItem>, AppError> {
    current_user(&state, &headers).await?;
    let item = state
        .trace_store
        .get_capability(id)
        .await?
        .filter(|c| c.kind == crate::trace::CapabilityKind::Shortcut)
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(item))
}

pub(super) async fn update_shortcut(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdateCapabilityRequest>,
) -> Result<Json<crate::trace::CapabilityItem>, AppError> {
    if let Some(name) = payload.name.as_deref() {
        if !name.trim().is_empty() && !shortcut_name_valid(name.trim()) {
            return Err(StatusCode::BAD_REQUEST.into());
        }
    }
    update_capability(
        crate::trace::CapabilityKind::Shortcut,
        id,
        &state,
        &headers,
        payload,
    )
    .await
}

pub(super) async fn delete_shortcut(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    delete_capability(crate::trace::CapabilityKind::Shortcut, id, &state, &headers).await
}
