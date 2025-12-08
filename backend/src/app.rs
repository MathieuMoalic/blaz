use crate::routes::auth;
use crate::{
    logging::{access_log, log_payloads},
    models::AppState,
    routes::{app_state, meal_plan, parse_recipe, recipes, shopping},
};

use axum::middleware::from_fn;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};

use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::services::ServeDir;

async fn healthz() -> Json<&'static str> {
    Json("ok")
}

fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}

pub fn build_app(state: AppState) -> Router {
    let media_service = ServeDir::new(state.config.media_dir.clone());

    // Request-ID middleware comes first so everything downstream
    // has access to the x-request-id header.
    let request_id_layer = ServiceBuilder::new()
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(PropagateRequestIdLayer::x_request_id());

    Router::new()
        .route("/healthz", get(healthz))
        .route(
            "/app-state",
            get(app_state::get)
                .patch(app_state::patch)
                .put(app_state::patch),
        )
        .route("/recipes", get(recipes::list).post(recipes::create))
        .route(
            "/recipes/{id}",
            get(recipes::get)
                .delete(recipes::delete)
                .patch(recipes::update),
        )
        .route("/recipes/{id}/image", post(recipes::upload_image))
        .route(
            "/recipes/{id}/macros/estimate",
            post(recipes::estimate_macros),
        )
        .route("/recipes/import", post(parse_recipe::import_from_url))
        .route(
            "/meal-plan",
            get(meal_plan::get_for_day).post(meal_plan::assign),
        )
        .route("/meal-plan/{day}/{recipe_id}", delete(meal_plan::unassign))
        .route("/shopping", get(shopping::list).post(shopping::create))
        .route(
            "/shopping/{id}",
            patch(shopping::patch_shopping_item).delete(shopping::delete),
        )
        .route("/shopping/merge", post(shopping::merge_items))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/status", get(auth::auth_status))
        .nest_service("/media", media_service)
        .with_state(state)
        .layer(request_id_layer)
        .layer(from_fn(access_log))
        .layer(from_fn(log_payloads))
        .layer(cors_layer())
}
