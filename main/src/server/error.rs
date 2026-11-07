//! Ошибка хендлеров HTTP API: код ответа или внутренний сбой, причина
//! которого уходит в лог, а не клиенту.

use std::fmt::Display;
use std::panic::Location;

use axum::{
    http::{header::InvalidHeaderValue, StatusCode},
    response::{IntoResponse, Response},
};

/// Ошибка хендлера. Хендлеры возвращают `Result<_, AppError>` и пробрасывают
/// ошибки через `?`:
/// - код ответа (`StatusCode`: 400/403/404/409…) уходит как есть, с пустым
///   телом;
/// - сбой хранилища или exec (`sqlx::Error` и др., см. `From` ниже) — ответ
///   `500 Internal Server Error` с пустым телом: детали наружу не отдаются,
///   причина и место (`файл:строка:столбец` оператора `?`) пишутся в лог
///   через `tracing::error!`.
#[derive(Debug)]
pub enum AppError {
    /// Ответ с этим кодом и пустым телом.
    Status(StatusCode),
    /// Внутренний сбой: клиенту — 500 без деталей, причина — в лог.
    Internal {
        cause: String,
        at: &'static Location<'static>,
    },
}

impl AppError {
    /// Внутренний сбой с причиной для лога. Место — вызов этой функции, а
    /// из `From` — оператор `?` в хендлере (`#[track_caller]`).
    #[track_caller]
    pub fn internal(cause: impl Display) -> Self {
        Self::Internal {
            cause: cause.to_string(),
            at: Location::caller(),
        }
    }

    /// Та же ошибка для ручек, отвечающих текстом `(StatusCode, String)`
    /// (проверка инструментов MCP-сервера): код прежний, тело — пустой текст;
    /// причина внутреннего сбоя — в лог, как у `into_response`.
    pub fn into_text(self) -> (StatusCode, String) {
        (self.into_status(), String::new())
    }

    /// Код ответа; причина внутреннего сбоя пишется в лог.
    fn into_status(self) -> StatusCode {
        match self {
            Self::Status(code) => code,
            Self::Internal { cause, at } => {
                tracing::error!("internal error at {at}: {cause}");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        self.into_status().into_response()
    }
}

/// Код ответа — тот же код с пустым телом.
impl From<StatusCode> for AppError {
    fn from(code: StatusCode) -> Self {
        Self::Status(code)
    }
}

/// Сбой БД (TraceStore/ChatStore).
impl From<sqlx::Error> for AppError {
    #[track_caller]
    fn from(e: sqlx::Error) -> Self {
        Self::internal(e)
    }
}

/// Не подписался connection-JWT Centrifugo.
impl From<crate::centrifuge::CentrifugeError> for AppError {
    #[track_caller]
    fn from(e: crate::centrifuge::CentrifugeError) -> Self {
        Self::internal(e)
    }
}

/// Сбой git в воркстейшне (страница Changes).
impl From<crate::git_changes::ChangesError> for AppError {
    #[track_caller]
    fn from(e: crate::git_changes::ChangesError) -> Self {
        Self::internal(e)
    }
}

/// Не собрался заголовок ответа (Set-Cookie во входе через SSO).
impl From<InvalidHeaderValue> for AppError {
    #[track_caller]
    fn from(e: InvalidHeaderValue) -> Self {
        Self::internal(e)
    }
}

/// Чтение содержимого проекта: недопустимый путь — 400, пути нет — 404,
/// сбой самого exec (нет kubectl/docker) — 500.
impl From<crate::project_files::FileError> for AppError {
    #[track_caller]
    fn from(e: crate::project_files::FileError) -> Self {
        use crate::project_files::FileError;
        match e {
            FileError::InvalidPath(_) => Self::Status(StatusCode::BAD_REQUEST),
            FileError::NotFound => Self::Status(StatusCode::NOT_FOUND),
            FileError::Exec(_) => Self::internal(e),
        }
    }
}
