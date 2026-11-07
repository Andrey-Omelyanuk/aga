//! Воркстейшны и сессии на них, просмотр содержимого и изменений проекта в
//! станции (только чтение), публичный SSH-ключ aga и трасса задач агента.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header::CONTENT_TYPE, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use super::{current_user, publish_lifecycle, AppError, AppState};
use crate::chat::{Chat, SessionError};
use crate::cluster::Cluster;

pub(super) async fn get_trace(
    Path(task_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    current_user(&state, &headers).await?;
    match state.trace_store.get_trace(&task_id).await? {
        Some(trace) => Ok(Json(serde_json::to_value(trace).unwrap())),
        None => Err(StatusCode::NOT_FOUND.into()),
    }
}

pub(super) async fn list_workstations(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::chat::Workstation>>, AppError> {
    current_user(&state, &headers).await?;
    Ok(Json(state.chat_store.list_workstations().await?))
}

#[derive(Deserialize)]
pub struct CreateWorkstationRequest {
    #[serde(default)]
    pub name: Option<String>,
    /// Имя k8s-Secret из кластера, монтируемого в под при подъёме.
    #[serde(default)]
    pub secret: Option<String>,
}

/// Создание воркстейшна — только для суперпользователя (админ внешний,
/// интерфейс станции не создаёт и не удаляет). Участники получают 403.
/// Станция поднимается свободной и пустой: проект разворачивается на ней
/// при открытии сессии (`POST /workstations/:id/session`).
pub(super) async fn create_workstation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateWorkstationRequest>,
) -> Result<Json<crate::chat::Workstation>, AppError> {
    let user_id = current_user(&state, &headers).await?;
    if !state
        .chat_store
        .is_super_user(user_id)
        .await
        .unwrap_or(false)
    {
        return Err(StatusCode::FORBIDDEN.into());
    }

    let name = payload.name.unwrap_or_else(|| "ws".to_string());

    // SSH-ключ aga (env): прокидывается в воркстейшн автоматически. Явный
    // `secret` из запроса (имя k8s-Secret) имеет приоритет; иначе при
    // заданном ключе используется общий `aga-ssh`.
    let ssh_key = crate::ssh_key::private_key_from_env();
    let effective_secret = match (payload.secret.clone(), ssh_key.as_ref()) {
        (Some(secret), _) => Some(secret),
        (None, Some(_)) => Some(crate::ssh_key::SSH_SECRET_NAME.to_string()),
        (None, None) => None,
    };

    // Воркстейшн — под в Kubernetes: сначала запись, потом сам под.
    let ws = state
        .chat_store
        .create_workstation(&name, effective_secret.as_deref())
        .await?;

    // Станция пустая: GIT_URL не задан — entrypoint инициализирует пустое
    // git-репо в /work/project, код проекта развернёт открытие сессии.
    let pod_name = Cluster::pod_name(ws.id);
    let branch = Cluster::branch_name(ws.id);

    // Ключ до подъёма станции: в k8s — Secret в кластере (монтируется в под
    // манифестом, поэтому создаётся до пода). В dev-режиме (docker) ключ
    // инжектится после `create_pod` — контейнер должен существовать.
    if let (Some(key), crate::cluster::Backend::K8s) = (ssh_key.as_ref(), state.cluster.backend) {
        if let Err(e) = state
            .cluster
            .ensure_ssh_secret(crate::ssh_key::SSH_SECRET_NAME, key)
            .await
        {
            let _ = state
                .chat_store
                .set_workstation_state(ws.id, "failed")
                .await;
            return Err(AppError::internal(format!(
                "failed to ensure ssh secret {}: {e}",
                crate::ssh_key::SSH_SECRET_NAME
            )));
        }
    }

    if let Err(e) = state
        .cluster
        .create_pod(&pod_name, "", &branch, ws.secret.as_deref())
        .await
    {
        let _ = state
            .chat_store
            .set_workstation_state(ws.id, "failed")
            .await;
        return Err(AppError::internal(format!(
            "failed to create workstation pod {pod_name}: {e}"
        )));
    }

    // dev-режим: ключ в ~/.ssh существующего контейнера (созданного выше или
    // переиспользованного из compose-стенда). Сбой инъекции не роняет станцию.
    if let (Some(key), crate::cluster::Backend::Docker) = (ssh_key.as_ref(), state.cluster.backend)
    {
        if let Err(e) = state.cluster.inject_ssh_key(&pod_name, key).await {
            tracing::error!("failed to inject ssh key into {pod_name}: {e}");
        }
    }

    // Под уже создан; готовность (Running + пустое git-репо) подтягиваем
    // ожиданием — под тянет образ, это занимает время. Проект на станции
    // появится при открытии сессии.
    if state.cluster.wait_ready(&pod_name).await.unwrap_or(false) {
        let _ = state.chat_store.set_workstation_state(ws.id, "ready").await;
    }

    let created = state
        .chat_store
        .get_workstation(ws.id)
        .await?
        .ok_or_else(|| AppError::internal(format!("created workstation {} not found", ws.id)))?;
    Ok(Json(created))
}

pub(super) async fn delete_workstation(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let user_id = current_user(&state, &headers).await?;
    if !state
        .chat_store
        .is_super_user(user_id)
        .await
        .unwrap_or(false)
    {
        return Err(StatusCode::FORBIDDEN.into());
    }
    // Сначала под, потом запись: если кластер не отдал под — состояние не
    // трогаем, пользователь повторит.
    if let Err(e) = state.cluster.delete_pod(&Cluster::pod_name(id)).await {
        return Err(AppError::internal(format!(
            "failed to delete workstation pod ws-{id}: {e}"
        )));
    }
    if state.chat_store.delete_workstation(id).await? {
        Ok(StatusCode::OK)
    } else {
        Err(StatusCode::NOT_FOUND.into())
    }
}

/// Отпустить воркстейшн — только суперпользователь. Свободная станция (без
/// открытой сессии): файлы проекта очищаются до пустого git-репо; сам ws
/// (под/сервис) не пересоздаётся.
pub(super) async fn release_workstation(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::chat::Workstation>, AppError> {
    let user_id = current_user(&state, &headers).await?;
    if !state
        .chat_store
        .is_super_user(user_id)
        .await
        .unwrap_or(false)
    {
        return Err(StatusCode::FORBIDDEN.into());
    }
    let ws = state
        .chat_store
        .release_workstation(id)
        .await
        .map_err(|e| match e {
            SessionError::NotFound => StatusCode::NOT_FOUND.into(),
            SessionError::WorkstationBusy => StatusCode::CONFLICT.into(),
            e => AppError::internal(e),
        })?;
    let executor = crate::workstation::executor_for_workstation(Some(id), &state.cluster);
    if let Err(e) = crate::ws_ops::release_workspace(&executor).await {
        tracing::warn!("failed to clear workspace on released ws-{id}: {e}");
    }
    Ok(Json(ws))
}

/// Отметить упавший воркстейшн как недоступный (инициирует восстановление).
pub(super) async fn mark_workstation_down(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let user_id = current_user(&state, &headers).await?;
    if !state
        .chat_store
        .is_super_user(user_id)
        .await
        .unwrap_or(false)
    {
        return Err(StatusCode::FORBIDDEN.into());
    }
    if state.chat_store.get_workstation(id).await?.is_none() {
        return Err(StatusCode::NOT_FOUND.into());
    }
    state.chat_store.mark_workstation_down(id).await?;
    Ok(StatusCode::OK)
}

// === Просмотр содержимого проекта в воркстейшне ===

/// Относительный путь внутри проекта; пустой — корень.
#[derive(Deserialize)]
pub struct ProjectFilePathQuery {
    #[serde(default)]
    pub path: String,
}

/// Дерево папок и файлов проекта воркстейшна. Читается напрямую из
/// под/контейнера (exec find) и доступно любому вошедшему участнику —
/// персональной видимости нет (см. `can_read`). Ошибки чтения — по
/// `From<FileError> for AppError`: путь 400, нет пути 404, сбой exec 500.
pub(super) async fn workstation_tree(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ProjectFilePathQuery>,
) -> Result<Json<crate::project_files::Tree>, AppError> {
    current_user(&state, &headers).await?;
    if state.chat_store.get_workstation(id).await?.is_none() {
        return Err(StatusCode::NOT_FOUND.into());
    }
    let executor = crate::workstation::executor_for_workstation(Some(id), &state.cluster);
    let tree =
        crate::project_files::tree(&executor, crate::project_files::PROJECT_ROOT, &params.path)
            .await?;
    Ok(Json(tree))
}

/// Содержимое файла: текст (text/plain, подсветка на фронте) или байты
/// с MIME (картинки/видео/аудио). Только чтение — записывать нельзя.
pub(super) async fn workstation_file(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ProjectFilePathQuery>,
) -> Result<Response, AppError> {
    current_user(&state, &headers).await?;
    if state.chat_store.get_workstation(id).await?.is_none() {
        return Err(StatusCode::NOT_FOUND.into());
    }
    let executor = crate::workstation::executor_for_workstation(Some(id), &state.cluster);
    let content =
        crate::project_files::read(&executor, crate::project_files::PROJECT_ROOT, &params.path)
            .await?;
    Ok(file_response(content))
}

/// Изменения проекта воркстейшна от начала ветки (страница Changes). Дифф
/// берётся из git внутри воркстейшна; доступен любому вошедшему участнику,
/// как и просмотр содержимого. Только чтение: коммиты и push не выполняются.
pub(super) async fn workstation_changes(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::git_changes::ChangesSummary>, AppError> {
    current_user(&state, &headers).await?;
    if state.chat_store.get_workstation(id).await?.is_none() {
        return Err(StatusCode::NOT_FOUND.into());
    }
    let executor = crate::workstation::executor_for_workstation(Some(id), &state.cluster);
    let summary =
        crate::git_changes::changes(&executor, crate::project_files::PROJECT_ROOT).await?;
    Ok(Json(summary))
}

/// Публичный SSH-ключ aga (общий на инстанс) для страницы «Настройки».
/// Приватный ключ задаёт админ в env `AGA_SSH_PRIVATE_KEY`; здесь отдаётся
/// только публичный (не секрет) — для добавления в deploy-ключи репозитория.
#[derive(Serialize)]
pub struct SshKeyInfo {
    /// Есть ли настроенный ключ (env задан).
    configured: bool,
    /// Публичный ключ в OpenSSH-формате (`ssh-ed25519 AAAA...`).
    public_key: Option<String>,
    /// SHA256-fingerprint (`SHA256:...`).
    fingerprint: Option<String>,
}

pub(super) async fn get_ssh_key(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SshKeyInfo>, AppError> {
    current_user(&state, &headers).await?;
    let Some(private_key) = crate::ssh_key::private_key_from_env() else {
        return Ok(Json(SshKeyInfo {
            configured: false,
            public_key: None,
            fingerprint: None,
        }));
    };
    match crate::ssh_key::derive_public_key(&private_key) {
        Ok((public_key, fingerprint)) => Ok(Json(SshKeyInfo {
            configured: true,
            public_key: Some(public_key),
            fingerprint: Some(fingerprint),
        })),
        Err(e) => Err(AppError::internal(format!(
            "invalid {}: {e}",
            crate::ssh_key::SSH_PRIVATE_KEY_ENV
        ))),
    }
}

fn file_response(content: crate::project_files::FileContent) -> Response {
    if content.mime.starts_with("text/") {
        let text = String::from_utf8_lossy(&content.bytes).to_string();
        let mut resp = (StatusCode::OK, text).into_response();
        resp.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        resp
    } else {
        let mut resp = Response::new(Body::from(content.bytes));
        if let Ok(hv) = HeaderValue::from_str(&content.mime) {
            resp.headers_mut().insert(CONTENT_TYPE, hv);
        }
        resp
    }
}

#[derive(Deserialize)]
pub struct OpenWorkstationSessionRequest {
    pub project_id: i64,
    #[serde(default)]
    pub title: Option<String>,
}

/// Активная сессия воркстейшна (или null).
pub(super) async fn get_workstation_session(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Option<Chat>>, AppError> {
    current_user(&state, &headers).await?;
    let chat_id = state.chat_store.active_session_id(id).await?;
    let Some(chat_id) = chat_id else {
        return Ok(Json(None));
    };
    let chat = state.chat_store.get_chat(chat_id).await?;
    Ok(Json(chat))
}

/// Открыть сессию на воркстейшне: любой участник, воркстейшн готов,
/// открытой сессии на нём нет. Станция обслуживает сессии: проект задаётся
/// здесь и разворачивается в `/work/project` (код проекта станция до этого
/// не знает). Закрытие сессии освобождает воркстейшн.
pub(super) async fn open_workstation_session(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<OpenWorkstationSessionRequest>,
) -> Result<Json<Chat>, AppError> {
    let user_id = current_user(&state, &headers).await?;
    let git_url = match state.trace_store.get_project(payload.project_id).await? {
        Some(p) => p.git_url,
        None => return Err(StatusCode::NOT_FOUND.into()),
    };
    let chat = match state
        .chat_store
        .open_workstation_session(id, payload.project_id, payload.title.as_deref(), user_id)
        .await
    {
        Ok(chat) => chat,
        Err(SessionError::NotFound) => return Err(StatusCode::NOT_FOUND.into()),
        Err(SessionError::WorkstationNotReady) => return Err(StatusCode::CONFLICT.into()),
        Err(SessionError::WorkstationBusy) => return Err(StatusCode::CONFLICT.into()),
        Err(SessionError::Forbidden) => return Err(StatusCode::FORBIDDEN.into()),
        Err(SessionError::Db(e)) => return Err(e.into()),
    };
    let executor = crate::workstation::executor_for_workstation(Some(id), &state.cluster);
    let branch = Cluster::branch_name(id);
    // SSH-ключ aga для git+ssh-клона: в k8s-поде ключ приходит из Secret при
    // создании, в dev (docker) контейнеры ws поднимаются заранее и
    // переиспользуются — ключ инжектится перед клонированием проекта.
    if let (Some(key), crate::cluster::Backend::Docker) = (
        crate::ssh_key::private_key_from_env().as_ref(),
        state.cluster.backend,
    ) {
        if let Err(e) = state
            .cluster
            .inject_ssh_key(&Cluster::pod_name(id), key)
            .await
        {
            tracing::warn!("failed to inject ssh key into ws-{id}: {e}");
        }
    }
    // Развернуть код проекта на станции — лучшая попытка: в dev-стенде git_url
    // может быть плейсхолдером (станция работает с примонтированной копией и
    // проект наполняет агент), поэтому сбой exec не откатывает сессию, а
    // только логируется.
    if let Err(e) = crate::ws_ops::replace_project(&executor, &git_url, &branch).await {
        tracing::warn!("failed to deploy project {git_url} on ws-{id}: {e}");
    }
    // Ручное восстановление после падения: если открытая сессия — продолжение
    // прерванной на упавшей станции, восстанавливаем файлы проекта из её ветки.
    // Сбой восстановления сессию не роняет — станция остаётся на чистом клоне,
    // ошибка уходит в лог.
    if let Ok(Some(prev)) = state.chat_store.continues_session_id(chat.id).await {
        if let Ok(Some(prev_branch)) = state.chat_store.session_branch(prev).await {
            if let Err(e) = crate::ws_ops::restore_workspace(&executor, &prev_branch).await {
                tracing::warn!("failed to restore session from ws into ws-{id}: {e}");
            }
        }
    }
    publish_lifecycle(&state, "session_opened", chat.id).await;
    Ok(Json(chat))
}
