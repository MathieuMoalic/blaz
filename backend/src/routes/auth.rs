use crate::error::AppResult; // NEW
use crate::models::AppState;
use argon2::Argon2;
use axum::{Json, extract::State, http::StatusCode};
use jsonwebtoken::{Algorithm, Header, encode};
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Deserialize)]
pub struct RegisterReq {
    pub email: String,
    pub password: String,
}
#[derive(Deserialize)]
pub struct LoginReq {
    pub email: String,
    pub password: String,
}
#[derive(Serialize)]
pub struct LoginResp {
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: i64,
    exp: u64,
}
fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Register the single allowed user account.
///
/// # Errors
/// Returns an error if:
/// - A user already exists (registration disabled).
/// - The provided email or password is invalid.
/// - Checking for existing users fails.
/// - Password hashing fails.
/// - Inserting the new user fails (including unique constraint conflicts).
pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterReq>,
) -> AppResult<StatusCode> {
    // --- SINGLE-USER GUARD ------------------------------------
    // If there is already any user in the database, forbid registration.
    let existing_user = sqlx::query("SELECT 1 FROM users LIMIT 1")
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if existing_user.is_some() {
        // Frontend already treats 403 as "Registration is disabled."
        return Err(StatusCode::FORBIDDEN.into());
    }
    // -----------------------------------------------------------

    let email = req.email.trim();
    let password = req.password.trim();
    if email.is_empty() || !email.contains('@') || password.len() < 8 {
        return Err(StatusCode::BAD_REQUEST.into());
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();

    let resp = sqlx::query("INSERT INTO users (email, password_hash) VALUES (?, ?)")
        .bind(email)
        .bind(hash)
        .execute(&state.pool)
        .await;

    match resp {
        Ok(_) => Ok(StatusCode::CREATED),
        Err(e) => {
            if let sqlx::Error::Database(db) = &e {
                if db.is_unique_violation() {
                    return Err(StatusCode::CONFLICT.into());
                }
            }
            Err(StatusCode::INTERNAL_SERVER_ERROR.into())
        }
    }
}

#[derive(Serialize)]
pub struct AuthStatusResp {
    pub allow_registration: bool,
}

/// Report whether registration is currently allowed.
///
/// # Errors
/// Returns an error if querying the users table fails.
pub async fn auth_status(State(state): State<AppState>) -> AppResult<Json<AuthStatusResp>> {
    let existing_user = sqlx::query("SELECT 1 FROM users LIMIT 1")
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(AuthStatusResp {
        // Registration allowed only when there are no users at all.
        allow_registration: existing_user.is_none(),
    }))
}

/// Authenticate a user and return a JWT token.
///
/// # Errors
/// Returns an error if:
/// - The email is not found or the password is invalid.
/// - Querying the user record fails.
/// - Parsing or verifying the stored password hash fails.
/// - Encoding the JWT fails.
pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginReq>,
) -> AppResult<Json<LoginResp>> {
    // CHANGED
    let row = sqlx::query("SELECT id, password_hash FROM users WHERE email = ?")
        .bind(req.email.trim())
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let uid: i64 = row.get(0);
    let stored: String = row.get(1);

    let parsed = PasswordHash::new(&stored).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if Argon2::default()
        .verify_password(req.password.as_bytes(), &parsed)
        .is_err()
    {
        return Err(StatusCode::UNAUTHORIZED.into());
    }

    let exp = now_ts() + 7 * 24 * 3600;
    let token = encode(
        &Header::new(Algorithm::HS256),
        &Claims { sub: uid, exp },
        &state.jwt_encoding,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(LoginResp { token }))
}
