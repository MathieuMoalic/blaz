pub mod config;
pub mod db;
pub mod error;
pub mod image_io;
pub mod ingredient_parser;
pub mod models;
pub mod routes;
pub mod units;

use crate::{
    models::AppState,
    routes::{app_state, meal_plan, parse_recipe, recipes, shopping},
};
use axum::body::Body;
use axum::http::{Request, Response, header};
use axum::middleware::{Next, from_fn};
use axum::{
    Json, Router,
    extract::ConnectInfo,
    routing::{delete, get, patch, post},
};
use routes::auth;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::{
    classify::ServerErrorsFailureClass,
    cors::{Any, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::{Span, info_span};

pub fn init_logging() {
    use tracing_subscriber::{
        EnvFilter, fmt::time::UtcTime, layer::SubscriberExt, util::SubscriberInitExt,
    };
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_timer(UtcTime::rfc_3339())
                .compact(),
        )
        .init();
}

async fn healthz() -> Json<&'static str> {
    Json("ok")
}

/// Logs request & response bodies (dev-friendly).
/// Skips multipart requests and likely-binary responses, truncates previews.
/// Now includes the request-id for correlation.
async fn log_payloads(req: Request<Body>, next: Next) -> Response<Body> {
    // Capture request-id (inserted by SetRequestIdLayer)
    let req_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string();

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
                tracing::info!(request_id=%req_id, request_body=%preview, "request body");
                Request::from_parts(req_parts, Body::from(bytes))
            }
            Err(e) => {
                tracing::warn!(request_id=%req_id, error=%e, "failed reading request body");
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
                tracing::info!(request_id=%req_id, response_body=%preview, "response body");
                Response::from_parts(res_parts, Body::from(bytes))
            }
            Err(e) => {
                tracing::warn!(request_id=%req_id, error=%e, "failed reading response body");
                Response::from_parts(res_parts, Body::empty())
            }
        }
    } else {
        Response::from_parts(res_parts, res_body)
    }
}

fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}

pub fn build_app(state: AppState) -> Router {
    let trace = TraceLayer::new_for_http()
        .make_span_with(|req: &Request<Body>| {
            let method = req.method().to_string();
            let uri = req.uri().to_string();
            let client_ip = req
                .extensions()
                .get::<ConnectInfo<std::net::SocketAddr>>()
                .map(|ci| ci.0.to_string())
                .unwrap_or_else(|| "-".into());
            let rid = req
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("-");

            info_span!("http", method=%method, uri=%uri, client_ip=%client_ip, request_id=%rid)
        })
        .on_request(|_req: &Request<Body>, _span: &Span| {
            tracing::info!("request started");
        })
        .on_response(|res: &Response<Body>, latency: Duration, _span: &Span| {
            tracing::info!(status=%res.status(), latency_ms=%latency.as_millis(), "response completed");
        })
        .on_failure(|_class: ServerErrorsFailureClass, latency: Duration, _span: &Span| {
            tracing::error!(latency_ms=%latency.as_millis(), "request failed");
        });

    let media_service = ServeDir::new(state.config.media_dir.clone());

    // Request-ID middleware comes first so everything downstream (logging/tracing)
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
        .layer(from_fn(log_payloads))
        .layer(cors_layer())
        .layer(trace)
}
