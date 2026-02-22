use axum::{Json, extract::State, http::StatusCode};
use axum::extract::Multipart;
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use std::time::Duration;

use crate::error::AppResult;
use crate::llm::{ImageChatRequest, LlmClient};
use crate::models::{AppState, NewRecipe, Recipe};
use crate::routes::{parse_recipe::ExtractRaw, recipes};

const MAX_IMAGES: usize = 3;
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024; // 10 MB per image

/// Import a recipe from up to 3 photos using the configured vision LLM.
///
/// Accepts a multipart form with fields named `image` (repeat up to 3×).
///
/// # Errors
///
/// Returns an error if the API key is missing, the LLM call fails, or the
/// multipart payload cannot be parsed.
pub async fn import_from_images(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> AppResult<Json<Recipe>> {
    let token = state.config.llm_api_key.clone().unwrap_or_default();
    if token.is_empty() {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "LLM API key is not configured".into(),
        )
            .into());
    }

    // Collect up to MAX_IMAGES images from the multipart body
    let mut images: Vec<(String, String)> = Vec::new(); // (mime, base64)

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (StatusCode::BAD_REQUEST, format!("multipart error: {e}"))
    })? {
        if images.len() >= MAX_IMAGES {
            break;
        }

        let mime = field
            .content_type()
            .map_or_else(|| "image/jpeg".to_string(), ToString::to_string);

        let bytes = field.bytes().await.map_err(|e| {
            (StatusCode::BAD_REQUEST, format!("read error: {e}"))
        })?;

        if bytes.len() > MAX_IMAGE_BYTES {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("image exceeds {} MB limit", MAX_IMAGE_BYTES / 1024 / 1024),
            )
                .into());
        }

        images.push((mime, B64.encode(&bytes)));
    }

    if images.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "no images provided".into()).into());
    }

    let model = &state.config.llm_vision_model;
    let base = state.config.llm_api_url.as_str();
    let system = state.config.system_prompt_import.as_str();
    let prompt = "Extract the recipe from the image(s). \
                  If multiple images are provided they show different parts of the same recipe. \
                  Return the combined recipe as JSON.";

    let http = reqwest::Client::new();
    let llm = LlmClient::new(base.to_string(), token, model.clone());

    let llm_json = llm
        .chat_json_images(ImageChatRequest {
            http: &http,
            system,
            text_prompt: prompt,
            images: &images,
            temperature: 0.1,
            timeout: Duration::from_secs(120),
            max_tokens: Some(5000),
        })
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("vision LLM failed: {e}")))?;

    let raw = ExtractRaw::from_json(&llm_json);
    let title = raw
        .title
        .clone()
        .unwrap_or_else(|| "Imported recipe".to_string());
    let norm = raw.normalize();

    let payload = NewRecipe {
        title,
        source: String::new(),
        r#yield: String::new(),
        notes: String::new(),
        ingredients: norm.ingredients,
        instructions: norm.instructions,
    };

    let created = recipes::create(State(state.clone()), Json(payload)).await?;
    let recipe_id = created.0.id;
    let fresh = recipes::get(State(state), axum::extract::Path(recipe_id)).await?;
    Ok(fresh)
}
