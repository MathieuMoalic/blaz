use axum::{Json, extract::State};
use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::AppState;
use crate::error::AppResult;

#[derive(Serialize)]
pub struct LlmCredits {
    /// USD spent so far on this key.
    pub usage: f64,
    /// Credit limit in USD, or null for pay-as-you-go / unlimited.
    pub limit: Option<f64>,
    pub is_free_tier: bool,
}

/// Fetch credit/usage info from the configured LLM provider.
/// Only works with `OpenRouter` (`/auth/key` endpoint).
///
/// # Errors
/// Returns an error if the API key is not set, the request fails, or the
/// provider does not expose a `/auth/key` endpoint.
pub async fn get(State(state): State<AppState>) -> AppResult<Json<LlmCredits>> {
    let Some(ref api_key) = state.config.llm_api_key else {
        return Err(crate::error::AppError::Msg(
            axum::http::StatusCode::BAD_REQUEST,
            "LLM API key is not configured".into(),
        ));
    };
    if api_key.trim().is_empty() {
        return Err(crate::error::AppError::Msg(
            axum::http::StatusCode::BAD_REQUEST,
            "LLM API key is not configured".into(),
        ));
    }

    let base = state.config.llm_api_url.trim_end_matches('/');
    let url = format!("{base}/auth/key");

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Credits request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Credits endpoint returned {status}: {body}").into());
    }

    let body: JsonValue = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Invalid credits response: {e}"))?;

    let data = body.get("data").unwrap_or(&body);

    let usage = data
        .get("usage")
        .and_then(JsonValue::as_f64)
        .unwrap_or(0.0);
    let limit = data.get("limit").and_then(JsonValue::as_f64);
    let is_free_tier = data
        .get("is_free_tier")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);

    Ok(Json(LlmCredits {
        usage,
        limit,
        is_free_tier,
    }))
}
