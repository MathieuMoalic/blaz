use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{error::AppResult, models::AppState};

/// Get all settings
pub async fn get_all(State(state): State<AppState>) -> AppResult<Json<HashMap<String, String>>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM settings")
            .fetch_all(&state.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let map: HashMap<String, String> = rows.into_iter().collect();
    Ok(Json(map))
}

#[derive(Deserialize)]
pub struct UpdateSettings {
    pub settings: HashMap<String, String>,
}

#[derive(Serialize)]
pub struct UpdateResponse {
    pub updated: usize,
}

/// Update multiple settings at once
pub async fn update(
    State(state): State<AppState>,
    Json(req): Json<UpdateSettings>,
) -> AppResult<Json<UpdateResponse>> {
    let mut updated = 0;

    for (key, value) in req.settings {
        // Only allow known settings keys
        if !is_valid_setting_key(&key) {
            continue;
        }

        sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)")
            .bind(&key)
            .bind(&value)
            .execute(&state.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        updated += 1;
    }

    Ok(Json(UpdateResponse { updated }))
}

fn is_valid_setting_key(key: &str) -> bool {
    matches!(
        key,
        "llm_model" | "llm_fallback_model" | "llm_vision_model" | "llm_vision_fallback_model"
    )
}

/// Helper to get a setting value from the database
pub async fn get_setting(pool: &sqlx::SqlitePool, key: &str) -> Option<String> {
    sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

/// LLM settings struct for convenient access
#[derive(Clone, Debug)]
pub struct LlmSettings {
    pub model: String,
    pub fallback_model: String,
    pub vision_model: String,
    pub vision_fallback_model: String,
}

impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            model: "google/gemini-2.0-flash-001".to_string(),
            fallback_model: "openai/gpt-4o-mini".to_string(),
            vision_model: "google/gemini-2.0-flash-001".to_string(),
            vision_fallback_model: "openai/gpt-4o-mini".to_string(),
        }
    }
}

impl LlmSettings {
    /// Load LLM settings from database, falling back to defaults
    pub async fn load(pool: &sqlx::SqlitePool) -> Self {
        let defaults = Self::default();

        Self {
            model: get_setting(pool, "llm_model")
                .await
                .filter(|s| !s.is_empty())
                .unwrap_or(defaults.model),
            fallback_model: get_setting(pool, "llm_fallback_model")
                .await
                .filter(|s| !s.is_empty())
                .unwrap_or(defaults.fallback_model),
            vision_model: get_setting(pool, "llm_vision_model")
                .await
                .filter(|s| !s.is_empty())
                .unwrap_or(defaults.vision_model),
            vision_fallback_model: get_setting(pool, "llm_vision_fallback_model")
                .await
                .filter(|s| !s.is_empty())
                .unwrap_or(defaults.vision_fallback_model),
        }
    }
}
