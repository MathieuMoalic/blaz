use crate::llm::LlmClient;
use axum::{
    Json,
    extract::{Multipart, Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use serde::Deserialize;
use sqlx::Arguments;
use sqlx::sqlite::SqliteArguments;
use std::fmt::Write as _;
use tracing::error;

use crate::models::RecipeMacros;
use crate::models::{AppState, NewRecipe, Recipe, RecipeRow, UpdateRecipe};

use crate::error::AppResult;

use std::io;

async fn store_recipe_image_bytes(
    state: &AppState,
    recipe_id: i64,
    bytes: Vec<u8>,
) -> anyhow::Result<(String, String)> {
    let (full_webp, thumb_webp) =
        tokio::task::spawn_blocking(move || -> io::Result<(Vec<u8>, Vec<u8>)> {
            let img = image::load_from_memory(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("decode error: {e}")))?;
            crate::image_io::to_full_and_thumb_webp(&img)
        })
        .await??;

    let rel_dir = format!("recipes/{recipe_id}");
    let rel_full = format!("{rel_dir}/full.webp");
    let rel_small = format!("{rel_dir}/small.webp");

    let abs_dir = state.config.media_dir.join(&rel_dir);
    tokio::fs::create_dir_all(&abs_dir).await?;
    tokio::fs::write(abs_dir.join("full.webp"), &full_webp).await?;
    tokio::fs::write(abs_dir.join("small.webp"), &thumb_webp).await?;

    Ok((rel_full, rel_small))
}

/// Keep SELECT/RETURNING columns in one place to avoid drift with structs.
const RECIPE_COLS: &str = r#"
    id, title, source, "yield", notes,
    created_at, updated_at,
    ingredients, instructions,
    image_path_small, image_path_full,
    macros
"#;

/// # Errors
///
/// Err if request fails
pub async fn fetch_and_store_recipe_image(
    client: &reqwest::Client,
    abs_url: &str,
    state: &AppState,
    recipe_id: i64,
) -> anyhow::Result<(String, String)> {
    let bytes = client
        .get(abs_url)
        .header(reqwest::header::USER_AGENT, "blaz/recipe-importer")
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec();

    store_recipe_image_bytes(state, recipe_id, bytes).await
}

/// # Errors
///
/// Err if parsing of multipart fails
pub async fn upload_image(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> AppResult<Json<Recipe>> {
    let mut bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart.next_field().await? {
        if let Some("image" | "file") = field.name() {
            bytes = Some(field.bytes().await?.to_vec());
            break;
        }
    }

    // If nothing uploaded, return current recipe (preserve old behavior)
    let Some(bytes) = bytes else {
        let sql = format!("SELECT {RECIPE_COLS} FROM recipes WHERE id = ?");
        let recipe: Recipe = sqlx::query_as::<_, RecipeRow>(&sql)
            .bind(id)
            .fetch_one(&state.pool)
            .await?
            .into();
        return Ok(Json(recipe));
    };

    let (rel_full, rel_small) = store_recipe_image_bytes(&state, id, bytes).await?;

    // Update DB (store relative filenames)
    sqlx::query(
        r"
        UPDATE recipes
           SET image_path_full  = ?,
               image_path_small = ?,
               updated_at       = CURRENT_TIMESTAMP
         WHERE id = ?
        ",
    )
    .bind(&rel_full)
    .bind(&rel_small)
    .bind(id)
    .execute(&state.pool)
    .await?;

    // Return updated recipe
    let sql = format!("SELECT {RECIPE_COLS} FROM recipes WHERE id = ?");
    let recipe: Recipe = sqlx::query_as::<_, RecipeRow>(&sql)
        .bind(id)
        .fetch_one(&state.pool)
        .await?
        .into();

    Ok(Json(recipe))
}

/// # Errors
///
/// Err if querying the db fails
pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<Recipe>>> {
    let sql = format!("SELECT {RECIPE_COLS} FROM recipes ORDER BY id");
    let rows: Vec<RecipeRow> = sqlx::query_as::<_, RecipeRow>(&sql)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| {
            error!(?e, "recipes.list failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(rows.into_iter().map(Recipe::from).collect()))
}

/// # Errors
///
/// Err if querying the db fails
pub async fn get(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Recipe>> {
    let sql = format!("SELECT {RECIPE_COLS} FROM recipes WHERE id = ?");
    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(&sql)
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            error!(?e, ?id, "recipes.get failed");
            StatusCode::NOT_FOUND
        })?;

    Ok(Json(row.into()))
}

/// # Errors
///
/// Err if querying the db fails
pub async fn create(
    State(state): State<AppState>,
    Json(new): Json<NewRecipe>,
) -> AppResult<Json<Recipe>> {
    if new.title.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST.into());
    }

    // Strict validation for object-only ingredients
    for ing in &new.ingredients {
        if ing.name.trim().is_empty() {
            return Err(StatusCode::BAD_REQUEST.into());
        }
        if let Some(u) = ing.unit.as_deref() {
            if u.trim().is_empty() {
                return Err(StatusCode::BAD_REQUEST.into());
            }
        }
        if let Some(p) = ing.prep.as_deref() {
            if p.trim().is_empty() {
                return Err(StatusCode::BAD_REQUEST.into());
            }
        }
    }

    let ingredients_json = serde_json::to_string(&new.ingredients).unwrap_or_else(|_| "[]".into());
    let instructions_json =
        serde_json::to_string(&new.instructions).unwrap_or_else(|_| "[]".into());

    let sql = format!(
        r#"
        INSERT INTO recipes (title, source, "yield", notes, ingredients, instructions, created_at, updated_at)
        VALUES (?, ?, ?, ?, json(?), json(?), CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        RETURNING {RECIPE_COLS}
        "#
    );

    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(&sql)
        .bind(new.title)
        .bind(new.source)
        .bind(new.r#yield)
        .bind(new.notes)
        .bind(ingredients_json)
        .bind(instructions_json)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            error!(?e, "recipes.create failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(row.into()))
}

/// # Errors
///
/// Err if querying the db fails
pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    let res = sqlx::query(r"DELETE FROM recipes WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            error!(?e, "recipes.delete failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if res.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND.into());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// # Errors
///
/// Err if querying the db fails
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    payload: Result<Json<UpdateRecipe>, JsonRejection>,
) -> AppResult<Json<Recipe>> {
    let Json(up) = payload.map_err(|rejection| {
        let msg = rejection.body_text();
        tracing::error!("JSON deserialization failed in recipes::update: {}", msg);
        (StatusCode::UNPROCESSABLE_ENTITY, msg)
    })?;
    
    let mut sets: Vec<&'static str> = Vec::new();
    let mut args = SqliteArguments::default();

    if let Some(title) = up.title {
        sets.push("title = ?");
        args.add(title).map_err(|e| {
            error!(?e, "arg add (title) failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    if let Some(source) = up.source {
        sets.push("source = ?");
        args.add(source).map_err(|e| {
            error!(?e, "arg add (source) failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    if let Some(y) = up.r#yield {
        sets.push(r#""yield" = ?"#);
        args.add(y).map_err(|e| {
            error!(?e, "arg add (yield) failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    if let Some(notes) = up.notes {
        sets.push("notes = ?");
        args.add(notes).map_err(|e| {
            error!(?e, "arg add (notes) failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    if let Some(ings) = up.ingredients {
        for ing in &ings {
            if ing.name.trim().is_empty() {
                return Err(StatusCode::BAD_REQUEST.into());
            }
        }

        let s = serde_json::to_string(&ings).unwrap_or_else(|_| "[]".into());
        sets.push("ingredients = json(?)");
        args.add(s).map_err(|e| {
            error!(?e, "arg add (ingredients) failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    if let Some(instr) = up.instructions {
        let s = serde_json::to_string(&instr).unwrap_or_else(|_| "[]".to_string());
        sets.push("instructions = json(?)");
        args.add(s).map_err(|e| {
            error!(?e, "arg add (instructions) failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    sets.push("updated_at = CURRENT_TIMESTAMP");

    let sql = format!("UPDATE recipes SET {} WHERE id = ?", sets.join(", "));
    args.add(id).map_err(|e| {
        error!(?e, "arg add (id) failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let res = sqlx::query_with(&sql, args)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            error!(?e, "recipes.update failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if res.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND.into());
    }

    let sql = format!("SELECT {RECIPE_COLS} FROM recipes WHERE id = ?");
    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(&sql)
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            error!(?e, "recipes.get after update failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(row.into()))
}

/* ---------- Estimate & store macros ---------- */

fn servings_from_yield(y: &str) -> Option<f64> {
    let y = y.trim();
    if y.is_empty() {
        return None;
    }

    // Normalize decimals
    let y_norm = y.replace(',', ".");
    let y_lower = y_norm.to_ascii_lowercase();

    // Reject obvious non-serving yields, e.g. "500 g", "1 loaf"
    if crate::units::NON_SERVING_YIELD_RE.is_match(&y_lower) {
        return None;
    }

    // Allow if:
    // - the whole string is just a number/range
    // - OR it contains a servings hint ("serves", "people", "portions", "makes", ...)
    let looks_bare = crate::units::BARE_NUM_RANGE_RE.is_match(&y_lower);
    let has_hint = crate::units::SERVINGS_HINT_RE.is_match(&y_lower);

    if !looks_bare && !has_hint {
        return None;
    }

    // Extract first number/range using existing regex
    if let Some(cap) = crate::units::SERVINGS_NUM_RE.captures(&y_norm) {
        let a: f64 = cap.get(1)?.as_str().parse().ok()?;
        if let Some(bm) = cap.get(2) {
            let b: f64 = bm.as_str().parse().ok()?;
            return Some((a + b) / 2.0);
        }
        return Some(a);
    }

    None
}

/// # Errors
/// Returns an error if the recipe cannot be loaded, the LLM call fails,
/// the LLM response cannot be parsed, or the macros cannot be saved.
pub async fn estimate_macros(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Recipe>> {
    let row = load_recipe_row(&state, id).await?;
    let (servings, basis) = servings_and_basis(&row.r#yield);

    let user = build_macros_user_prompt(servings, &row);

    let client = macros_http_client()?;
    let sys = &state.config.system_prompt_macros;

    let macros = call_and_parse_macros_llm(&client, &state.config, sys, &user, basis).await?;

    save_macros(&state, id, &macros).await?;

    let final_row = load_recipe_row(&state, id).await?;
    Ok(Json(Recipe::from(final_row)))
}

/* ---------------- helpers ---------------- */

async fn load_recipe_row(state: &AppState, id: i64) -> AppResult<RecipeRow> {
    let sql = format!("SELECT {RECIPE_COLS} FROM recipes WHERE id = ?");
    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(&sql)
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(row)
}

fn servings_and_basis(y: &str) -> (Option<f64>, &'static str) {
    let servings = servings_from_yield(y);
    let basis = if servings.is_some() {
        "per_serving"
    } else {
        "per_recipe"
    };
    (servings, basis)
}

fn build_macros_user_prompt(servings: Option<f64>, row: &RecipeRow) -> String {
    let ingredients_lines = ingredient_lines(row);
    let instructions_lines = &row.instructions.0;

    let mut user = String::new();

    match servings {
        Some(sv) => {
            let _ = writeln!(user, "SERVINGS: {sv}");
        }
        None => {
            user.push_str("SERVINGS: unknown (return totals for the entire recipe)\n");
        }
    }

    user.push_str("\nINGREDIENTS:\n");
    for l in &ingredients_lines {
        let _ = writeln!(user, "- {l}");
    }

    if !instructions_lines.is_empty() {
        user.push_str("\nINSTRUCTIONS (may help disambiguate prep/cooking losses):\n");
        for (i, step) in instructions_lines.iter().enumerate() {
            let _ = writeln!(user, "{}. {step}", i + 1);
        }
    }

    user
}

fn ingredient_lines(row: &RecipeRow) -> Vec<String> {
    row.ingredients
        .0
        .iter()
        .map(|i| {
            let name = match i.prep.as_deref() {
                Some(p) if !p.trim().is_empty() => format!("{}, {}", i.name, p.trim()),
                _ => i.name.clone(),
            };

            match (i.quantity, i.unit.as_deref()) {
                (Some(q), Some(u)) if !u.is_empty() => format!("{q} {u} {name}"),
                (Some(q), _) => format!("{q} {name}"),
                _ => name,
            }
        })
        .collect()
}

fn macros_http_client() -> Result<reqwest::Client, StatusCode> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(25))
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn call_and_parse_macros_llm(
    client: &reqwest::Client,
    config: &crate::config::Config,
    sys: &str,
    user: &str,
    basis: &'static str,
) -> AppResult<RecipeMacros> {
    #[allow(clippy::struct_field_names)]
    #[derive(Deserialize)]
    struct LlmOutGrams {
        protein_g: f64,
        fat_g: f64,
        carbs_g: f64,
    }
    let base = &config.llm_api_url;
    let token = config.llm_api_key.clone().unwrap_or_default();
    let model = &config.llm_model;

    let llm = LlmClient::new(base.to_string(), token.clone(), model.to_string());

    let val = llm
        .chat_json(
            client,
            sys,
            user,
            0.1,
            std::time::Duration::from_secs(25),
            Some(500),
        )
        .await
        .map_err(|e| {
            error!(?e, "LLM call failed");
            StatusCode::BAD_GATEWAY
        })?;

    let parsed: LlmOutGrams = serde_json::from_value(val).map_err(|e| {
        error!(?e, "LLM JSON parse failed");
        StatusCode::BAD_GATEWAY
    })?;

    Ok(RecipeMacros {
        basis: basis.to_string(),
        protein_g: round1(parsed.protein_g),
        fat_g: round1(parsed.fat_g),
        carbs_g: round1(parsed.carbs_g),
    })
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

async fn save_macros(state: &AppState, id: i64, macros: &RecipeMacros) -> AppResult<()> {
    let macros_json = serde_json::to_string(macros).unwrap_or_else(|_| "{}".into());

    sqlx::query(
        r"
        UPDATE recipes
           SET macros = json(?),
               updated_at = CURRENT_TIMESTAMP
         WHERE id = ?
        ",
    )
    .bind(macros_json)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        error!(?e, "recipes.update macros failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(())
}
