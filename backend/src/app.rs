use crate::routes::auth;
use crate::{
    auth_middleware::require_auth,
    config::Config,
    embedded_web::serve_embedded_web,
    logging::{access_log, log_payloads},
    models::AppState,
    routes::{import_recipe_images, import_recipesage, llm_credits, meal_plan, parse_recipe, recipes, share_recipe, shopping},
};

use axum::extract::DefaultBodyLimit;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde::Serialize;

use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::services::ServeDir;

async fn healthz() -> Json<&'static str> {
    Json("ok")
}

#[derive(Serialize)]
struct VersionInfo {
    version: &'static str,
}

async fn version() -> Json<VersionInfo> {
    Json(VersionInfo {
        version: env!("CARGO_PKG_VERSION"),
    })
}

fn cors_layer(config: &Config) -> CorsLayer {
    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_headers(Any);

    if let Some(origin) = &config.cors_origin {
        // Specific origin for production
        cors.allow_origin(
            origin
                .parse::<axum::http::HeaderValue>()
                .expect("Invalid CORS origin")
        )
    } else {
        // Allow any origin (development only)
        tracing::warn!("CORS configured to allow any origin - not secure for production!");
        cors.allow_origin(Any)
    }
}

#[allow(clippy::needless_pass_by_value)] // Axum requires AppState ownership
pub fn build_app(state: AppState) -> Router {
    let media_service = ServeDir::new(state.config.media_dir.clone());

    let request_id_layer = ServiceBuilder::new()
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(PropagateRequestIdLayer::x_request_id());

    // Public routes (no authentication required)
    let public_routes = Router::new()
        .route("/healthz", get(healthz))
        .route("/version", get(version))
        .route("/auth/login", post(auth::login))
        .route("/api/share/{token}", get(share_recipe::get_shared_recipe));

    // Protected routes (authentication required)
    let protected_routes = Router::new()
        .route("/recipes", get(recipes::list).post(recipes::create))
        .route(
            "/recipes/{id}",
            get(recipes::get)
                .delete(recipes::delete)
                .patch(recipes::update),
        )
        .route("/recipes/{id}/image", post(recipes::upload_image))
        .route("/recipes/{id}/share", post(share_recipe::create_share_token).delete(share_recipe::revoke_share_token))
        .route(
            "/recipes/{id}/macros/estimate",
            post(recipes::estimate_macros),
        )
        .route("/recipes/import", post(parse_recipe::import_from_url))
        .route("/recipes/import/images", post(import_recipe_images::import_from_images))
        .route("/recipes/import/recipesage", post(import_recipesage::import_recipesage))
        .route(
            "/meal-plan",
            get(meal_plan::get_for_day).post(meal_plan::assign),
        )
        .route("/meal-plan/{day}/{recipe_id}", delete(meal_plan::unassign))
        .route("/shopping", get(shopping::list).post(shopping::create))
        .route("/shopping/all-texts", get(shopping::list_all_texts))
        .route(
            "/shopping/{id}",
            patch(shopping::patch_shopping_item).delete(shopping::delete),
        )
        .route("/shopping/merge", post(shopping::merge_items))
        .route("/llm/credits", get(llm_credits::get))
        .route_layer(from_fn_with_state(state.clone(), require_auth));

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .nest_service("/media", media_service)
        .fallback(serve_embedded_web)
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024)) // 50MB for large imports
        .layer(request_id_layer)
        .layer(from_fn(access_log))
        .layer(from_fn(log_payloads))
        .layer(cors_layer(&state.config))
}
