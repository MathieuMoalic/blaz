pub mod db;
pub mod error;
pub mod ingredient_parser;
pub mod models;
pub mod routes;

use crate::{
    models::AppState,
    routes::{auth_meta, meal_plan, parse_recipe, recipes, shopping},
};
use axum::body::Body;
use axum::http::{HeaderValue, Method, Request, Response, header};
use axum::middleware::{Next, from_fn};
use axum::{
    Json, Router,
    extract::ConnectInfo,
    routing::{delete, get, patch, post},
};
use routes::auth;
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

/// Logs request & response bodies (dev-friendly).
/// Skips multipart requests and likely-binary responses, truncates previews.
async fn log_payloads(req: Request<Body>, next: Next) -> Response<Body> {
    // Decide if we log the request body (avoid multipart)
    let req_ct = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Split parts/body
    let (req_parts, req_body) = req.into_parts();
    let req = if !req_ct.starts_with("multipart/") {
        match axum::body::to_bytes(req_body, 64 * 1024).await {
            Ok(bytes) => {
                let preview = if bytes.len() > 16 * 1024 {
                    format!(
                        "{}… [truncated]",
                        String::from_utf8_lossy(&bytes[..16 * 1024])
                    )
                } else {
                    String::from_utf8_lossy(&bytes).to_string()
                };
                tracing::info!(request_body = %preview, "request body");
                Request::from_parts(req_parts, Body::from(bytes))
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed reading request body");
                Request::from_parts(req_parts, Body::empty())
            }
        }
    } else {
        Request::from_parts(req_parts, req_body)
    };

    // Call handler
    let res: Response<Body> = next.run(req).await;

    // Response body logging decision
    let res_ct = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let (res_parts, res_body) = res.into_parts();
    if !res_ct.starts_with("image/") && !res_ct.starts_with("application/octet-stream") {
        match axum::body::to_bytes(res_body, 64 * 1024).await {
            Ok(bytes) => {
                let preview = if bytes.len() > 16 * 1024 {
                    format!(
                        "{}… [truncated]",
                        String::from_utf8_lossy(&bytes[..16 * 1024])
                    )
                } else {
                    String::from_utf8_lossy(&bytes).to_string()
                };
                tracing::info!(response_body = %preview, "response body");
                Response::from_parts(res_parts, Body::from(bytes))
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed reading response body");
                Response::from_parts(res_parts, Body::empty())
            }
        }
    } else {
        Response::from_parts(res_parts, res_body)
    }
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
            // PROD-ish — comma-separated domains supported
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

    // Serve /media from MEDIA_DIR
    let media_service = ServeDir::new(state.media_dir.clone());

    Router::new()
        .route("/healthz", get(healthz))
        // Recipes
        .route("/recipes", get(recipes::list).post(recipes::create))
        .route(
            "/recipes/{id}",
            get(recipes::get)
                .delete(recipes::delete)
                .patch(recipes::update),
        )
        .route("/recipes/{id}/image", post(recipes::upload_image))
        .route("/recipes/import", post(parse_recipe::import_from_url))
        // Meal plan
        .route(
            "/meal-plan",
            get(meal_plan::get_for_day).post(meal_plan::assign),
        )
        .route("/meal-plan/{day}/{recipe_id}", delete(meal_plan::unassign))
        // Shopping
        .route("/shopping", get(shopping::list).post(shopping::create))
        .route(
            "/shopping/{id}",
            patch(shopping::toggle_done).delete(shopping::delete),
        )
        .route("/shopping/merge", post(shopping::merge_items))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        // Auth meta (for frontend to decide showing Register)
        .route("/auth/meta", get(auth_meta::meta))
        // Static media
        .nest_service("/media", media_service)
        .with_state(state)
        .layer(from_fn(log_payloads))
        .layer(cors)
        .layer(trace)
}
