use axum::{Json, extract::State, http::StatusCode};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::AppState;
use crate::{
    error::AppResult,
    models::{AppSettings, SettingsRow},
};

fn default_llm_model() -> String {
    "deepseek/deepseek-chat-v3.1".to_string()
}

fn default_llm_api_url() -> String {
    "https://openrouter.ai/api/v1".to_string()
}

fn default_system_prompt_import() -> String {
    r#"You are a precise recipe data extractor and normalizer.

INPUT: plain text from a recipe page (any language).
OUTPUT: STRICT JSON with exactly these keys:
{"ingredients":[{"quantity":null|number,"unit":null|"g"|"kg"|"ml"|"L"|"tsp"|"tbsp","name":string}], "instructions":[]}

TASK:
- Translate to English.
- Convert ALL imperial units to metric in the INGREDIENTS.
  * Allowed units in ingredients: g, kg, ml, L, tsp, tbsp.
  * Never use: cup, cups, oz, ounce, ounces, fl oz, pound, lb.
  * Keep tsp and tbsp abbreviations as written (do not spell out).
- Keep amounts as numbers; keep ranges by converting both ends (e.g., 2–4 cups → 480–960 ml).
- For solid items, convert oz→g (1 oz ≈ 28 g). For liquids, convert fl oz→ml (1 fl oz ≈ 30 ml). For cups→ml (1 cup ≈ 240 ml).
- If an ingredient has prep words (e.g., sliced, diced, minced), put them AFTER the ingredient name, separated by ", " (comma + space).
  Example: "2 carrots, diced".
- If data is missing, return an empty array for that key.
- Do NOT include commentary or extra keys.
- When a quantity is a range, replace the range with the mean value of the range.
- Round quantities sensibly.
- Use 0.5/0.25/0.75 style; never 1/2, 1/4, etc.
- If no numeric quantity, set "quantity": null and "unit": null, keep the name.
- instructions: array of steps (strings). No commentary.

FORMAT EXAMPLES:
{"ingredients":["2 cloves garlic, minced","150 g flour","2 carrots, diced"],"instructions":["Cook the garlic.","Fold in flour."]}

SELF-CHECK:
Before answering, verify no banned units appear in INGREDIENTS. If any do, fix them and re-check. Answer only with the final JSON."#.to_string()
}

fn default_system_prompt_macros() -> String {
    r#"You are a precise nutrition estimator.

Return STRICT JSON with the following keys, all numeric grams with up to 1 decimal:
{
  "protein_g": number,
  "fat_g": number,     // saturated + unsaturated combined
  "carbs_g": number    // carbohydrates EXCLUDING fiber
}

Rules:
- Use common nutrition databases and reasonable approximations.
- Always include ALL three keys.
- Carbs exclude fiber (i.e., net carbs).
- If servings are provided, compute PER SERVING. Otherwise, compute for the ENTIRE RECIPE.
- Never add extra fields or commentary."#
        .to_string()
}

fn default_allow_registration() -> bool {
    true
}

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
#[serde(default)]
pub struct AppSettingsDto {
    pub llm_api_key: Option<String>,

    // use explicit default functions for clarity
    #[serde(default = "default_llm_model")]
    pub llm_model: String,

    #[serde(default = "default_llm_api_url")]
    pub llm_api_url: String,

    #[serde(default = "default_allow_registration")]
    pub allow_registration: bool,

    #[serde(default = "default_system_prompt_import")]
    pub system_prompt_import: String,

    #[serde(default = "default_system_prompt_macros")]
    pub system_prompt_macros: String,
}

impl Default for AppSettingsDto {
    fn default() -> Self {
        Self {
            llm_api_key: None,
            llm_model: default_llm_model(),
            llm_api_url: default_llm_api_url(),
            allow_registration: default_allow_registration(),
            system_prompt_import: default_system_prompt_import(),
            system_prompt_macros: default_system_prompt_macros(),
        }
    }
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
