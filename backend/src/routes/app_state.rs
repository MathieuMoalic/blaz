use axum::{Json, extract::State, http::StatusCode};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::AppState;
use crate::{
    error::AppResult,
    models::{AppSettings, SettingsRow},
};

#[derive(Serialize)]
pub struct AppStateView {
    pub llm_api_key_masked: String,
    pub llm_model: String,
    pub llm_api_url: String,
    pub allow_registration: bool,
    pub system_prompt_import: String,
    pub system_prompt_macros: String,
}

fn mask_key(k: &Option<String>) -> String {
    match k {
        None => "".into(),
        Some(s) if s.is_empty() => "".into(),
        Some(s) if s.len() <= 6 => "***".into(),
        Some(s) => {
            let end = &s[s.len().saturating_sub(4)..];
            format!("***{}", end)
        }
    }
}

pub async fn get(State(state): State<AppState>) -> AppResult<Json<AppStateView>> {
    let st = state.settings.read().await.clone();
    Ok(Json(AppStateView {
        llm_api_key_masked: mask_key(&st.llm_api_key),
        llm_model: st.llm_model,
        llm_api_url: st.llm_api_url,
        allow_registration: st.allow_registration,
        system_prompt_import: st.system_prompt_import,
        system_prompt_macros: st.system_prompt_macros,
    }))
}

#[derive(Deserialize, Default)]
pub struct PatchAppState {
    pub llm_api_key: Option<String>, // set to "" to clear
    pub llm_model: Option<String>,
    pub llm_api_url: Option<String>,
    pub allow_registration: Option<bool>,
    pub system_prompt_import: Option<String>,
    pub system_prompt_macros: Option<String>,
}

pub async fn patch(
    State(state): State<AppState>,
    Json(p): Json<PatchAppState>,
) -> AppResult<Json<AppStateView>> {
    // 1) Apply to DB (singleton row id=1)
    let mut current: SettingsRow = sqlx::query_as::<_, SettingsRow>(
        r#"
        SELECT llm_api_key, llm_model, llm_api_url, allow_registration,
               system_prompt_import, system_prompt_macros
          FROM settings WHERE id = 1
        "#,
    )
    .fetch_one(&state.pool)
    .await?;

    if let Some(v) = p.llm_api_key {
        current.llm_api_key = if v.trim().is_empty() { None } else { Some(v) };
    }
    if let Some(v) = p.llm_model {
        current.llm_model = v;
    }
    if let Some(v) = p.llm_api_url {
        current.llm_api_url = v;
    }
    if let Some(v) = p.allow_registration {
        current.allow_registration = if v { 1 } else { 0 };
    }
    if let Some(v) = p.system_prompt_import {
        current.system_prompt_import = v;
    }
    if let Some(v) = p.system_prompt_macros {
        current.system_prompt_macros = v;
    }

    sqlx::query(
        r#"
        UPDATE settings
           SET llm_api_key = ?,
               llm_model = ?,
               llm_api_url = ?,
               allow_registration = ?,
               system_prompt_import = ?,
               system_prompt_macros = ?
         WHERE id = 1
        "#,
    )
    .bind(&current.llm_api_key)
    .bind(&current.llm_model)
    .bind(&current.llm_api_url)
    .bind(current.allow_registration)
    .bind(&current.system_prompt_import)
    .bind(&current.system_prompt_macros)
    .execute(&state.pool)
    .await?;

    // 2) Update in-memory settings
    {
        let mut guard = state.settings.write().await;
        *guard = AppSettings::from(current);
    }

    // 3) Return masked view
    get(State(state)).await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettingsDto {
    pub llm_api_key: Option<String>,
    pub llm_model: String,
    pub llm_api_url: String,
    pub allow_registration: bool,
    pub system_prompt_import: String,
    pub system_prompt_macros: String,
}

// Global, in-memory settings store.
static SETTINGS: OnceCell<RwLock<AppSettingsDto>> = OnceCell::new();

pub async fn update_app_state(
    State(_app): State<AppState>,
    Json(payload): Json<AppSettingsDto>,
) -> Result<Json<AppSettingsDto>, (StatusCode, String)> {
    let lock = SETTINGS
        .get()
        .expect("SETTINGS not initialized; call init_from_env() at startup");

    {
        let mut s = lock.write().await;
        // Replace everything (frontend sends the full object)
        *s = payload.clone();
    }

    // Note: If you want /auth/meta to reflect `allow_registration` live,
    // have that endpoint read from SETTINGS instead of a fixed bool on AppState.

    Ok(Json(payload))
}
