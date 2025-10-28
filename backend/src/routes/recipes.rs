use crate::image_io::to_full_and_thumb_webp;
use crate::{
    ingredient_parser::parse_ingredient_line,
    models::{AppState, Ingredient, NewRecipe, Recipe, RecipeMacros, RecipeRow, UpdateRecipe},
};
use axum::{
    Json,
    extract::{Multipart, Path, State},
    http::StatusCode,
};
use sqlx::Arguments;
use sqlx::sqlite::SqliteArguments;
use tracing::error;

use crate::error::AppResult;

use serde::Deserialize;

/* ---------- Shared LLM helper (OpenAI-compatible HF Router) ---------- */

async fn call_llm_json(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    model: &str,
    system: &str,
    user: &str,
) -> anyhow::Result<serde_json::Value> {
    use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "temperature": 0.1,
        "max_tokens": 500,
        "response_format": {"type":"json_object"}
    });

    let mut req = client
        .post(format!("{}/chat/completions", base))
        .header(CONTENT_TYPE, "application/json");

    if !token.is_empty() {
        req = req.header(AUTHORIZATION, format!("Bearer {}", token));
    }

    let resp = req.json(&body).send().await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        anyhow::bail!("llm router {}: {}", status, text);
    }

    #[derive(Deserialize)]
    struct ChoiceMsg {
        content: String,
    }
    #[derive(Deserialize)]
    struct Choice {
        message: ChoiceMsg,
    }
    #[derive(Deserialize)]
    struct ChatResp {
        choices: Vec<Choice>,
    }

    let parsed: ChatResp = serde_json::from_str(&text)?;
    let content = parsed
        .choices
        .first()
        .ok_or_else(|| anyhow::anyhow!("no choices"))?
        .message
        .content
        .trim()
        .to_string();

    // Try parse as JSON straight; if model wrapped extra text, attempt to extract first {...}
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
        return Ok(val);
    }
    // crude fallback: find first object
    fn find_first_json_object(s: &str) -> Option<String> {
        let bytes = s.as_bytes();
        let mut start = None;
        let mut depth = 0i32;
        let mut in_str = false;
        let mut esc = false;
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'\\' if in_str => {
                    esc = !esc;
                    continue;
                }
                b'"' if !esc => {
                    in_str = !in_str;
                }
                b'{' if !in_str => {
                    if start.is_none() {
                        start = Some(i)
                    }
                    depth += 1;
                }
                b'}' if !in_str && depth > 0 => {
                    depth -= 1;
                    if depth == 0 {
                        let st = start?;
                        return Some(s[st..=i].to_string());
                    }
                }
                _ => {
                    esc = false;
                }
            }
        }
        None
    }
    if let Some(obj) = find_first_json_object(&content) {
        let val: serde_json::Value = serde_json::from_str(&obj)?;
        return Ok(val);
    }
    anyhow::bail!("model did not return JSON: {}", content)
}

/* ---------- Image upload & standard recipe routes ---------- */

pub async fn fetch_and_store_recipe_image(
    client: &reqwest::Client,
    abs_url: &str,
    state: &crate::models::AppState,
    recipe_id: i64,
) -> anyhow::Result<(String, String)> {
    use std::io;

    // 1) download
    let bytes = client
        .get(abs_url)
        .header(reqwest::header::USER_AGENT, "blaz/recipe-importer")
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec();

    // 2) decode + encode off-thread
    let (full_webp, thumb_webp) =
        tokio::task::spawn_blocking(move || -> io::Result<(Vec<u8>, Vec<u8>)> {
            let img = image::load_from_memory(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("decode error: {e}")))?;

            to_full_and_thumb_webp(&img)
        })
        .await??;

    // 3) write files
    tokio::fs::create_dir_all(&state.media_dir).await?;
    let uid = uuid::Uuid::new_v4();
    let full_name = format!("recipe_{recipe_id}_{uid}.webp");
    let thumb_name = format!("recipe_{recipe_id}_{uid}_sm.webp");
    tokio::fs::write(state.media_dir.join(&full_name), &full_webp).await?;
    tokio::fs::write(state.media_dir.join(&thumb_name), &thumb_webp).await?;

    Ok((full_name, thumb_name))
}

pub async fn upload_image(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> AppResult<Json<Recipe>> {
    use uuid::Uuid;

    // 1) Pull the file bytes (accept "image" or "file")
    let mut bytes: Option<Vec<u8>> = None;
    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("image") | Some("file") => {
                bytes = Some(field.bytes().await?.to_vec());
                break;
            }
            _ => continue,
        }
    }

    // If nothing uploaded, just return the current recipe state
    let bytes = match bytes {
        Some(b) => b,
        None => {
            let recipe: Recipe = sqlx::query_as::<_, RecipeRow>(
                r#"
                SELECT id, title, source, "yield", notes,
                       created_at, updated_at,
                       ingredients, instructions,
                       image_path, image_path_small, image_path_full,
                       macros
                  FROM recipes
                 WHERE id = ?
                "#,
            )
            .bind(id)
            .fetch_one(&state.pool)
            .await?
            .into();
            return Ok(Json(recipe));
        }
    };

    // 2) Ensure media dir
    tokio::fs::create_dir_all(&state.media_dir).await?;

    // 3) Build filenames (always .webp)
    let uid = Uuid::new_v4();
    let full_name = format!("recipe_{id}_{uid}.webp");
    let thumb_name = format!("recipe_{id}_{uid}_sm.webp");
    let full_abs = state.media_dir.join(&full_name);
    let thumb_abs = state.media_dir.join(&thumb_name);

    // 4) Heavy work off the async thread: decode, resize, encode to WebP
    let (full_webp, thumb_webp): (Vec<u8>, Vec<u8>) =
        tokio::task::spawn_blocking(move || -> std::io::Result<(Vec<u8>, Vec<u8>)> {
            // Decode any common image format
            let img = image::load_from_memory(&bytes).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, format!("decode error: {e}"))
            })?;

            to_full_and_thumb_webp(&img)
        })
        .await??;

    // 5) Write both files
    tokio::fs::write(&full_abs, &full_webp).await?;
    tokio::fs::write(&thumb_abs, &thumb_webp).await?;

    // 6) Update DB (store relative filenames)
    sqlx::query(
        r#"
        UPDATE recipes
           SET image_path_full  = ?,
               image_path_small = ?,
               updated_at       = CURRENT_TIMESTAMP
         WHERE id = ?
        "#,
    )
    .bind(&full_name)
    .bind(&thumb_name)
    .bind(id)
    .execute(&state.pool)
    .await?;

    // 7) Return updated recipe
    let recipe: Recipe = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions,
               image_path, image_path_small, image_path_full,
               macros
          FROM recipes
         WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?
    .into();

    Ok(Json(recipe))
}

pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<Recipe>>> {
    let rows: Vec<RecipeRow> = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions,
               image_path, image_path_small, image_path_full,
               macros
          FROM recipes
         ORDER BY id
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(rows.into_iter().map(Recipe::from).collect()))
}

pub async fn get(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Recipe>> {
    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions,
               image_path, image_path_small, image_path_full,
               macros
          FROM recipes
         WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok(Json(row.into()))
}

pub async fn create(
    State(state): State<AppState>,
    Json(new): Json<NewRecipe>,
) -> AppResult<Json<Recipe>> {
    if new.title.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST.into());
    }
    let structured: Vec<Ingredient> = new
        .ingredients
        .iter()
        .map(|s| parse_ingredient_line(s))
        .collect();
    let ingredients_json = serde_json::to_string(&structured).unwrap_or_else(|_| "[]".into());
    let instructions_json =
        serde_json::to_string(&new.instructions).unwrap_or_else(|_| "[]".into());

    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(r#"
    INSERT INTO recipes (title, source, "yield", notes, ingredients, instructions, created_at, updated_at)
    VALUES (?, ?, ?, ?, json(?), json(?), CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
    RETURNING id, title, source, "yield", notes,
              created_at, updated_at,
              ingredients, instructions,
              image_path, image_path_small, image_path_full,
              macros"#)
    .bind(new.title)
    .bind(new.source)
    .bind(new.r#yield)
    .bind(new.notes)
    .bind(ingredients_json)
    .bind(instructions_json)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(row.into()))
}

pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    let res = sqlx::query(r#"DELETE FROM recipes WHERE id = ?"#)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if res.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND.into());
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(up): Json<UpdateRecipe>,
) -> AppResult<Json<Recipe>> {
    let mut sets: Vec<&'static str> = Vec::new();
    let mut args = SqliteArguments::default();

    if let Some(title) = up.title {
        sets.push("title = ?");
        args.add(title)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    if let Some(source) = up.source {
        sets.push("source = ?");
        args.add(source)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    if let Some(y) = up.r#yield {
        sets.push(r#""yield" = ?"#);
        args.add(y).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    if let Some(notes) = up.notes {
        sets.push("notes = ?");
        args.add(notes)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    if let Some(ing_lines) = up.ingredients.as_ref() {
        let structured: Vec<Ingredient> =
            ing_lines.iter().map(|s| parse_ingredient_line(s)).collect();

        let s = serde_json::to_string(&structured).unwrap_or_else(|_| "[]".into());
        sets.push("ingredients = json(?)");
        args.add(s).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    if let Some(instr) = up.instructions {
        let s = serde_json::to_string(&instr).unwrap_or_else(|_| "[]".to_string());
        sets.push("instructions = json(?)");
        args.add(s).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    sets.push("updated_at = CURRENT_TIMESTAMP");

    let sql = format!("UPDATE recipes SET {} WHERE id = ?", sets.join(", "));
    args.add(id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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

    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions,
               image_path, image_path_small, image_path_full,
               macros
          FROM recipes
         WHERE id = ?
        "#,
    )
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

#[derive(Deserialize)]
struct LlmMacros {
    protein_g: f64,
    fat_g: f64,
    carbs_g: f64,
}

fn servings_from_yield(y: &str) -> Option<f64> {
    // Grab the first integer or decimal in the yield string.
    // Examples: "4 servings", "Serves 2", "2–3" -> 2.5
    use crate::units::DECIMAL_RE;
    if let Some(cap) = DECIMAL_RE.captures(y) {
        let a = cap.get(1)?.as_str().replace(',', ".");
        let a: f64 = a.parse().ok()?;
        let b = cap
            .get(2)
            .map(|m| m.as_str().replace(',', "."))
            .and_then(|s| s.parse::<f64>().ok());
        return Some(b.map(|bb| (a + bb) / 2.0).unwrap_or(a));
    }
    None
}

pub async fn estimate_macros(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Recipe>> {
    // Load recipe (ingredients + instructions + yield)
    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions,
               image_path, image_path_small, image_path_full,
               macros
          FROM recipes
         WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::NOT_FOUND)?;

    // Prepare prompt
    let servings = servings_from_yield(&row.r#yield);
    let basis = if servings.is_some() {
        "per_serving"
    } else {
        "per_recipe"
    };

    // Pretty ingredients for the model
    let ingredients_lines: Vec<String> = row
        .ingredients
        .0
        .iter()
        .map(|repr| match repr {
            crate::models::IngredientRepr::O(i) => match (i.quantity, i.unit.as_deref()) {
                (Some(q), Some(u)) if !u.is_empty() => format!("{q} {u} {}", i.name),
                (Some(q), _) => format!("{q} {}", i.name),
                _ => i.name.clone(),
            },
            crate::models::IngredientRepr::S(s) => s.clone(),
        })
        .collect();

    let instructions_lines = row.instructions.0;

    let system = r#"You are a precise nutrition estimator.

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
- Never add extra fields or commentary."#;

    let mut user = String::new();
    if let Some(sv) = servings {
        user.push_str(&format!("SERVINGS: {}\n", sv));
    } else {
        user.push_str("SERVINGS: unknown (return totals for the entire recipe)\n");
    }
    user.push_str("\nINGREDIENTS:\n");
    for l in &ingredients_lines {
        user.push_str("- ");
        user.push_str(l);
        user.push('\n');
    }
    if !instructions_lines.is_empty() {
        user.push_str("\nINSTRUCTIONS (may help disambiguate prep/cooking losses):\n");
        for (i, step) in instructions_lines.iter().enumerate() {
            user.push_str(&format!("{}. {}\n", i + 1, step));
        }
    }

    // Call LLM
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(25))
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let base = std::env::var("BLAZ_LLM_API_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "https://router.huggingface.co/v1".into());
    let token = std::env::var("BLAZ_LLM_API_KEY").unwrap_or_default();
    let model = std::env::var("BLAZ_LLM_MODEL")
        .ok()
        .unwrap_or_else(|| "meta-llama/Llama-3.1-8B-Instruct".into());

    let val = call_llm_json(&client, &base, &token, &model, system, &user)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let parsed: LlmMacros = serde_json::from_value(val).map_err(|_| StatusCode::BAD_GATEWAY)?;

    let macros = RecipeMacros {
        basis: basis.to_string(),
        protein_g: (parsed.protein_g * 10.0).round() / 10.0,
        fat_g: (parsed.fat_g * 10.0).round() / 10.0,
        carbs_g: (parsed.carbs_g * 10.0).round() / 10.0,
    };

    // Save to DB
    let macros_json = serde_json::to_string(&macros).unwrap_or_else(|_| "{}".into());
    sqlx::query(
        r#"
        UPDATE recipes
           SET macros = json(?),
               updated_at = CURRENT_TIMESTAMP
         WHERE id = ?
        "#,
    )
    .bind(macros_json)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Return updated recipe
    let final_row: RecipeRow = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions,
               image_path, image_path_small, image_path_full,
               macros
          FROM recipes
         WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(final_row.into()))
}
