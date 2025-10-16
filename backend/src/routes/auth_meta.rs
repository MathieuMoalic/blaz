use axum::{Json, extract::State};
use serde::Serialize;

use crate::{error::AppResult, models::AppState};

#[derive(Serialize)]
pub struct AuthMeta {
    pub allow_registration: bool,
}

/// GET /auth/meta  -> { "allow_registration": true/false }
pub async fn meta(State(state): State<AppState>) -> AppResult<Json<AuthMeta>> {
    Ok(Json(AuthMeta {
        allow_registration: state.allow_registration,
    }))
}
