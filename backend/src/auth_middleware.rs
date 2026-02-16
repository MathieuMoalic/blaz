use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

use crate::models::AppState;

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct Claims {
    sub: i64,
    exp: u64,
}

pub async fn require_auth(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract token from Authorization header
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Decode and verify JWT - clone jwt_secret to drop the lock immediately
    let jwt_secret = {
        let settings = state.settings.read().await;
        settings.jwt_secret.clone()
    };
    let decoding_key = DecodingKey::from_secret(jwt_secret.as_bytes());

    decode::<Claims>(token, &decoding_key, &Validation::new(Algorithm::HS256))
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    Ok(next.run(request).await)
}
