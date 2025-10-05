pub mod db;
pub mod error;
pub mod models;
pub mod routes;

use crate::{
    models::AppState,
    routes::{meal_plan, recipes, shopping},
};
use axum::{
    Json, Router,
    routing::{delete, get, patch},
};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

async fn healthz() -> Json<&'static str> {
    Json("ok")
}

pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/recipes", get(recipes::list).post(recipes::create))
        .route("/recipes/{id}", get(recipes::get).delete(recipes::delete))
        .route(
            "/meal-plan",
            get(meal_plan::get_for_day).post(meal_plan::assign),
        )
        .route("/meal-plan/{day}/{recipe_id}", delete(meal_plan::unassign))
        .route("/shopping", get(shopping::list).post(shopping::create))
        .route(
            "/shopping/{id}",
            patch(shopping::toggle_done).delete(shopping::delete),
        )
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http())
}
