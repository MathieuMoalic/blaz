use axum::{
    body::Body,
    http::{header, HeaderValue, Response, StatusCode, Uri},
    response::IntoResponse,
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web_build/"]
struct WebAssets;

pub async fn serve_embedded_web(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    
    // Try exact path first
    if let Some(content) = WebAssets::get(path) {
        return serve_asset(path, content.data.into_owned());
    }
    
    // For SPA routing, serve index.html for routes that don't match files
    if !path.contains('.') && let Some(content) = WebAssets::get("index.html") {
        return serve_asset("index.html", content.data.into_owned());
    }
    
    // Fallback to index.html
    if let Some(content) = WebAssets::get("index.html") {
        return serve_asset("index.html", content.data.into_owned());
    }
    
    // 404
    (StatusCode::NOT_FOUND, "Not found").into_response()
}

fn serve_asset(path: &str, content: Vec<u8>) -> Response<Body> {
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();
    
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&mime).unwrap_or(HeaderValue::from_static("application/octet-stream")),
        )
        .body(Body::from(content))
        .unwrap()
}
