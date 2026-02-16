use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::{error::AppResult, models::AppSettings};

#[derive(Serialize)]
pub struct AppStateView {
    pub llm_api_key_masked: String,
    pub llm_model: String,
    pub llm_api_url: String,
    pub system_prompt_import: String,
    pub system_prompt_macros: String,
}

fn mask_key(k: Option<&str>) -> String {
    match k {
        None | Some("") => String::new(),
        Some(s) if s.len() <= 6 => "***".to_string(),
        Some(s) => {
            let end = &s[s.len().saturating_sub(4)..];
            format!("***{end}")
        }
    }
}

/// Get the current application settings (with the LLM key masked).
///
/// # Errors
/// Returns an error if reading the settings from the database fails.
pub async fn get(State(state): State<AppState>) -> AppResult<Json<AppStateView>> {
    let st = state.settings.read().await.clone();
    Ok(Json(AppStateView {
        llm_api_key_masked: mask_key(st.llm_api_key.as_deref()),
        llm_model: st.llm_model,
        llm_api_url: st.llm_api_url,
        system_prompt_import: st.system_prompt_import,
        system_prompt_macros: st.system_prompt_macros,
    }))
}

#[derive(Deserialize, Default)]
pub struct PatchAppState {
    pub llm_api_key: Option<String>,
    pub llm_model: Option<String>,
    pub llm_api_url: Option<String>,
    pub system_prompt_import: Option<String>,
    pub system_prompt_macros: Option<String>,
}

/// Patch application settings (singleton row `id = 1`) and update the in-memory cache.
///
/// # Errors
/// Returns an error if:
/// - The settings row cannot be fetched from the database.
/// - The settings update query fails.
pub async fn patch(
    State(state): State<AppState>,
    Json(p): Json<PatchAppState>,
) -> AppResult<Json<AppStateView>> {
    tracing::debug!("patch: starting");
    
    // 1) Apply to DB (singleton row id=1)
    let mut settings = sqlx::query_as::<_, AppSettings>(
        r"
        SELECT llm_api_key, llm_model, llm_api_url,
               system_prompt_import, system_prompt_macros, jwt_secret
          FROM settings WHERE id = 1
        ",
    )
    .fetch_one(&state.pool)
    .await?;

    tracing::debug!("patch: fetched current settings");

    if let Some(v) = p.llm_api_key {
        settings.llm_api_key = if v.trim().is_empty() { None } else { Some(v) };
    }
    if let Some(v) = p.llm_model {
        settings.llm_model = v;
    }
    if let Some(v) = p.llm_api_url {
        settings.llm_api_url = v;
    }
    if let Some(v) = p.system_prompt_import {
        settings.system_prompt_import = v;
    }
    if let Some(v) = p.system_prompt_macros {
        settings.system_prompt_macros = v;
    }

    tracing::debug!("patch: updated settings struct, about to UPDATE db");

    sqlx::query(
        r"
        UPDATE settings
           SET llm_api_key = ?,
               llm_model = ?,
               llm_api_url = ?,
               system_prompt_import = ?,
               system_prompt_macros = ?,
               jwt_secret = ?
         WHERE id = 1
        ",
    )
    .bind(&settings.llm_api_key)
    .bind(&settings.llm_model)
    .bind(&settings.llm_api_url)
    .bind(&settings.system_prompt_import)
    .bind(&settings.system_prompt_macros)
    .bind(&settings.jwt_secret)
    .execute(&state.pool)
    .await?;

    tracing::debug!("patch: db updated, acquiring write lock");

    // 2) Update in-memory settings and prepare response
    let view = {
        *state.settings.write().await = settings.clone();
        tracing::debug!("patch: acquired write lock and updated");
        
        // Create response after dropping the lock
        AppStateView {
            llm_api_key_masked: mask_key(settings.llm_api_key.as_deref()),
            llm_model: settings.llm_model,
            llm_api_url: settings.llm_api_url,
            system_prompt_import: settings.system_prompt_import,
            system_prompt_macros: settings.system_prompt_macros,
        }
    };

    tracing::debug!("patch: returning response");
    Ok(Json(view))
}
