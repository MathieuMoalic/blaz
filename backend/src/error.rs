use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;

#[derive(Debug)]
pub enum AppError {
    /// Return just a status code with an empty body (preserves old behavior).
    Status(StatusCode),
    /// Return a status code with a plain-text message body.
    Msg(StatusCode, String),
    /// Internal error -> 500 with JSON body; logged.
    Anyhow(anyhow::Error),
}

impl From<StatusCode> for AppError {
    fn from(code: StatusCode) -> Self {
        Self::Status(code)
    }
}

impl From<(StatusCode, String)> for AppError {
    fn from((code, msg): (StatusCode, String)) -> Self {
        Self::Msg(code, msg)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        Self::Anyhow(e)
    }
}

/* ---- Narrow, explicit conversions so `?` works everywhere ---- */

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        Self::Anyhow(e.into())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Anyhow(e.into())
    }
}

impl From<axum::extract::multipart::MultipartError> for AppError {
    fn from(e: axum::extract::multipart::MultipartError) -> Self {
        Self::Anyhow(e.into())
    }
}

impl From<tokio::task::JoinError> for AppError {
    fn from(e: tokio::task::JoinError) -> Self {
        Self::Anyhow(e.into())
    }
}

#[derive(Serialize)]
struct ErrBody {
    error: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::Status(code) => code.into_response(), // empty body
            Self::Msg(code, msg) => (code, msg).into_response(), // preserves old tuple behavior
            Self::Anyhow(err) => {
                tracing::error!("{:#}", err);
                let body = Json(ErrBody {
                    error: err.to_string(),
                });
                (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
            }
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
