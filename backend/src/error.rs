use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;

#[derive(Debug)]
pub struct AppError(pub anyhow::Error);

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(e: E) -> Self {
        AppError(e.into())
    }
}

#[derive(Serialize)]
struct ErrBody {
    error: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        tracing::error!("{:#}", self.0);
        let body = Json(ErrBody {
            error: self.0.to_string(),
        });
        (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
