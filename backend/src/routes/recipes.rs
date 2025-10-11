use crate::{
    ingredient_parser::parse_ingredient_line,
    models::{AppState, Ingredient, NewRecipe, Recipe, RecipeRow, UpdateRecipe},
};
use axum::{
    Json,
    extract::{Multipart, Path, State},
    http::StatusCode,
};
use image::GenericImageView;
use sqlx::Arguments;
use sqlx::sqlite::SqliteArguments;
use tracing::error;

use crate::error::AppResult;

const FULL_WEBP_QUALITY: f32 = 90.0;
const THUMB_WEBP_QUALITY: f32 = 3.0;
const THUMB_MAX_DIM: u32 = 1024; // thumbnail bounding box (px)

pub async fn fetch_and_store_recipe_image(
    client: &reqwest::Client,
    abs_url: &str,
    state: &crate::models::AppState,
    recipe_id: i64,
) -> anyhow::Result<(String, String)> {
    use image::GenericImageView;
    use std::io;
    use webp::Encoder as WebpEncoder;

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

            let full_mem = WebpEncoder::from_image(&img)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("webp enc init: {e}")))?
                .encode(90.0);
            let (w, h) = img.dimensions();
            let thumb_img = if w <= 1024 && h <= 1024 {
                img
            } else {
                img.resize(1024, 1024, image::imageops::FilterType::Triangle)
            };
            let thumb_mem = WebpEncoder::from_image(&thumb_img)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("webp enc init: {e}")))?
                .encode(3.0);

            Ok((full_mem.to_vec(), thumb_mem.to_vec()))
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
    use image::imageops::FilterType;
    use uuid::Uuid;
    use webp::Encoder as WebpEncoder;

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
                       image_path, image_path_small, image_path_full
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

            // FULL (original size) -> WebP (quality)
            let full_mem = WebpEncoder::from_image(&img)
                .map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::Other, format!("webp enc init: {e}"))
                })?
                .encode(FULL_WEBP_QUALITY);
            let full_out = full_mem.to_vec();

            // THUMB: resize if needed, then WebP (lower quality)
            let (w, h) = img.dimensions();
            let thumb_img = if w <= THUMB_MAX_DIM && h <= THUMB_MAX_DIM {
                img
            } else {
                img.resize(THUMB_MAX_DIM, THUMB_MAX_DIM, FilterType::Triangle)
            };
            let thumb_mem = WebpEncoder::from_image(&thumb_img)
                .map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::Other, format!("webp enc init: {e}"))
                })?
                .encode(THUMB_WEBP_QUALITY);
            let thumb_out = thumb_mem.to_vec();

            Ok((full_out, thumb_out))
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
               image_path, image_path_small, image_path_full
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

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Recipe>>, StatusCode> {
    let rows: Vec<RecipeRow> = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions,
               image_path, image_path_small, image_path_full
          FROM recipes
         ORDER BY id
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(rows.into_iter().map(Recipe::from).collect()))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Recipe>, StatusCode> {
    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions,
               image_path, image_path_small, image_path_full
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
) -> Result<Json<Recipe>, StatusCode> {
    if new.title.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
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
              image_path, image_path_small, image_path_full"#)
    .bind(new.title)
    .bind(new.source)
    .bind(new.r#yield)
    .bind(new.notes)
    .bind(ingredients_json)    .bind(instructions_json)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(row.into()))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let res = sqlx::query(r#"DELETE FROM recipes WHERE id = ?"#)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if res.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(StatusCode::NO_CONTENT)
}
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(up): Json<UpdateRecipe>,
) -> Result<Json<Recipe>, StatusCode> {
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

    // ONLY parse when ingredients were provided
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

    // Always touch updated_at
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
        return Err(StatusCode::NOT_FOUND);
    }

    // Return fresh row
    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions,
               image_path, image_path_small, image_path_full
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
