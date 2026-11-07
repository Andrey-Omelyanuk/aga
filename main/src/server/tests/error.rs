//! `AppError`: код ответа уходит как есть, внутренний сбой — 500 без деталей
//! с причиной и местом для лога.

use super::*;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;

async fn body_text(resp: Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8_lossy(&bytes).to_string()
}

#[tokio::test]
async fn status_error_keeps_code_and_empty_body() {
    for code in [
        StatusCode::BAD_REQUEST,
        StatusCode::FORBIDDEN,
        StatusCode::NOT_FOUND,
        StatusCode::CONFLICT,
    ] {
        let resp = AppError::from(code).into_response();
        assert_eq!(resp.status(), code);
        assert!(resp.headers().get(CONTENT_TYPE).is_none());
        assert_eq!(body_text(resp).await, "");
    }
}

/// Сбой БД клиенту — 500 с пустым телом (как раньше), причина — только в лог.
#[tokio::test]
async fn internal_error_answers_500_without_details() {
    let resp = AppError::from(sqlx::Error::RowNotFound).into_response();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(resp.headers().get(CONTENT_TYPE).is_none());
    assert_eq!(body_text(resp).await, "");
}

/// Причина и место для лога: `?` в хендлере, а не внутренности `From`.
#[tokio::test]
async fn internal_error_keeps_cause_and_place_of_question_mark() {
    async fn db() -> Result<(), sqlx::Error> {
        Err(sqlx::Error::RowNotFound)
    }
    async fn handler() -> Result<(), AppError> {
        db().await?;
        Ok(())
    }
    match handler().await {
        Err(AppError::Internal { cause, at }) => {
            assert!(cause.contains("no rows"), "{cause}");
            assert!(at.file().ends_with("tests/error.rs"), "{at}");
        }
        other => panic!("ожидался внутренний сбой: {other:?}"),
    }
}

/// Ошибки чтения содержимого проекта: путь — 400, нет пути — 404, сбой
/// exec — внутренний (500).
#[test]
fn file_errors_map_to_status_codes() {
    use crate::project_files::FileError;
    let status = |e: FileError| match AppError::from(e) {
        AppError::Status(code) => code,
        AppError::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    };
    assert_eq!(
        status(FileError::InvalidPath("..".into())),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(status(FileError::NotFound), StatusCode::NOT_FOUND);
    assert_eq!(
        status(FileError::Exec("kubectl: not found".into())),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

/// Ручки с текстовыми ошибками получают тот же код и пустой текст.
#[test]
fn text_form_keeps_code_with_empty_text() {
    assert_eq!(
        AppError::from(StatusCode::NOT_FOUND).into_text(),
        (StatusCode::NOT_FOUND, String::new())
    );
    assert_eq!(
        AppError::from(sqlx::Error::RowNotFound).into_text(),
        (StatusCode::INTERNAL_SERVER_ERROR, String::new())
    );
}
