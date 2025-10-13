pub mod db;
pub mod error;
pub mod ingredient_parser;
pub mod models;
pub mod routes;

use crate::{
    models::AppState,
    routes::{meal_plan, parse_recipe, recipes, shopping},
};
use axum::http::{HeaderValue, Method, Request, Response, header};
use axum::{
    Json, Router,
    extract::ConnectInfo,
    routing::{delete, get, patch},
};
use axum::{body::Body, routing::post};
use std::time::Duration;
use tower_http::services::ServeDir;
use tower_http::{
    classify::ServerErrorsFailureClass,
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::{Span, info_span};

async fn healthz() -> Json<&'static str> {
    Json("ok")
}

pub fn build_app(state: AppState) -> Router {
    let trace = TraceLayer::new_for_http()
        .make_span_with(|req: &Request<Body>| {
            let method = req.method().to_string();
            let uri = req.uri().to_string();
            let ua = req
                .headers()
                .get(header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("-")
                .to_string();

            let client_ip = req
                .extensions()
                .get::<ConnectInfo<std::net::SocketAddr>>()
                .map(|ci| ci.0.to_string())
                .unwrap_or_else(|| "-".into());

            info_span!("http", method = %method, uri = %uri, client_ip = %client_ip, user_agent = %ua)
        })
        .on_request(|_req: &Request<Body>, _span: &Span| {
            tracing::info!("request started");
        })
        .on_response(|res: &Response<Body>, latency: Duration, _span: &Span| {
            tracing::info!(status = %res.status(), latency_ms = %latency.as_millis(), "response completed");
        })
        .on_failure(|_class: ServerErrorsFailureClass, latency: Duration, _span: &Span| {
            tracing::error!(latency_ms = %latency.as_millis(), "request failed");
        });

    // CORS: dev (no BLAZ_DOMAIN) = allow all; prod (BLAZ_DOMAIN set) = allowlist
    let cors = match std::env::var("BLAZ_DOMAIN") {
        Err(_) => {
            // DEV — permissive
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any)
        }
        Ok(domains) => {
            // PROD-ish — comma-separated domains supported, e.g. "app.example.com,admin.example.com"
            let origins: Vec<HeaderValue> = domains
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .flat_map(|d| [format!("https://{d}"), format!("http://{d}")])
                .filter_map(|s| s.parse::<HeaderValue>().ok())
                .collect();

            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PATCH,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
                .expose_headers([header::LOCATION])
                .max_age(std::time::Duration::from_secs(60 * 60))
        }
    };

    // Serve /media from MEDIA_DIR for both small & full images
    let media_service = ServeDir::new(state.media_dir.clone());

    Router::new()
        .route("/healthz", get(healthz))
        .route("/recipes", get(recipes::list).post(recipes::create))
        .route(
            "/recipes/{id}",
            get(recipes::get)
                .delete(recipes::delete)
                .patch(recipes::update),
        )
        .route("/recipes/{id}/image", post(recipes::upload_image))
        .route("/recipes/import", post(parse_recipe::import_from_url))
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
        .route("/shopping/merge", post(shopping::merge_items))
        .nest_service("/media", media_service)
        .with_state(state)
        .layer(cors)
        .layer(trace)
}
