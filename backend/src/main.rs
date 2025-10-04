use axum::{
    extract::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tower_http::{cors::{Any, CorsLayer}, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tokio::net::TcpListener; // <- add this

#[derive(Serialize, Deserialize, Clone)]
struct Recipe {
    id: u64,
    title: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "recipes_api=info,axum=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let recipes = vec![
        Recipe { id: 1, title: "Kig ha farz".into() },
        Recipe { id: 2, title: "Kouign-amann".into() },
    ];

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/recipes", get({
            let recipes = recipes.clone();
            move || {
                let recipes = recipes.clone();
                async move { Json(recipes) }
            }
        }))
        .route("/recipes", post(create_recipe))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("listening on http://{addr}");

    // axum 0.8 style:
    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn create_recipe(Json(mut r): Json<Recipe>) -> Json<Recipe> {
    if r.id == 0 { r.id = 999 }
    Json(r)
}

