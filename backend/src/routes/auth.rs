use crate::error::AppResult;
use crate::models::AppState;
use argon2::Argon2;
use axum::{Json, extract::State, http::StatusCode};
use jsonwebtoken::{Algorithm, Header, encode};
use password_hash::{PasswordHash, PasswordVerifier};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct LoginReq {
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResp {
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: i64, // Always 1 for single-user
    exp: u64,
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Authenticate with the configured password and return a JWT token.
///
/// # Errors
/// Returns an error if:
/// - No password hash is configured.
/// - The provided password is invalid.
/// - Parsing or verifying the password hash fails.
/// - Encoding the JWT fails.
pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginReq>,
) -> AppResult<Json<LoginResp>> {
    let stored_hash = state.config.password_hash.as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let parsed = PasswordHash::new(stored_hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if Argon2::default()
        .verify_password(req.password.as_bytes(), &parsed)
        .is_err()
    {
        return Err(StatusCode::UNAUTHORIZED.into());
    }

    let exp = now_ts() + 7 * 24 * 3600; // 7 days
    let token = encode(
        &Header::new(Algorithm::HS256),
        &Claims { sub: 1, exp },
        &state.jwt_encoding,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(LoginResp { token }))
}
