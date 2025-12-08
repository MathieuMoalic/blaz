use crate::config::Config;

use axum::body::Body;
use axum::http::{Request, Response, header};
use axum::middleware::Next;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Keep guards alive for the lifetime of the app.
pub struct LogGuards {
    _file_guard: Option<WorkerGuard>,
}

fn split_path(path: &Path) -> (PathBuf, String) {
    let dir = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let file = path
        .file_name()
        .unwrap_or_else(|| OsStr::new("blaz.log"))
        .to_string_lossy()
        .to_string();
    (dir, file)
}

#[must_use]
pub fn init_logging(config: &Config) -> LogGuards {
    let filter = EnvFilter::new(config.log_filter());

    // Stdout layer (pretty enough, ANSI enabled)
    let stdout_layer = fmt::layer()
        .with_target(false)
        .with_ansi(true)
        .compact()
        // requires tracing-subscriber "chrono" feature
        .with_timer(tracing_subscriber::fmt::time::ChronoLocal::new(
            "%Y-%m-%d %H:%M:%S".to_string(),
        ));

    // File layer (ANSI disabled)
    let (file_layer, guard) = {
        let path: &Path = config.log_file.as_ref();

        let (dir, file) = split_path(path);
        let appender = tracing_appender::rolling::never(dir, file);
        let (nb, guard) = tracing_appender::non_blocking(appender);

        let layer = fmt::layer()
            .with_target(false)
            .with_ansi(false)
            .compact()
            .with_timer(tracing_subscriber::fmt::time::ChronoLocal::new(
                "%Y-%m-%d %H:%M:%S".to_string(),
            ))
            .with_writer(nb);

        (Some(layer), Some(guard))
    };

    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer);

    if let Some(file_layer) = file_layer {
        subscriber.with(file_layer).init();
    } else {
        subscriber.init();
    }

    LogGuards { _file_guard: guard }
}

/// One-line access log.
/// 2xx/3xx -> INFO
/// 4xx/5xx -> ERROR (so stdout shows red by default ANSI level colors)
///
/// Includes query string.
pub async fn access_log(request: Request<Body>, next: Next) -> Response<Body> {
    let method = request.method().clone();

    // Clone the URI so we don't hold a borrow on `request`
    let uri = request.uri().clone();
    let path = uri
        .path_and_query()
        .map_or_else(|| uri.path().to_string(), |pq| pq.as_str().to_string());

    let res = next.run(request).await;
    let status = res.status().as_u16();

    // Padding for a slightly nicer aligned look
    let msg = format!("{method:<6} {path:<40} {status}");

    if (400..=599).contains(&status) {
        tracing::error!("{msg}");
    } else {
        tracing::info!("{msg}");
    }

    res
}

/// Logs request & response bodies (dev-friendly).
/// Skips multipart requests and likely-binary responses, truncates previews.
/// Includes request-id for correlation.
///
/// These logs are DEBUG so default verbosity stays clean.
///
/// Includes path+query in the structured fields.
pub async fn log_payloads(request: Request<Body>, next: Next) -> Response<Body> {
    // Capture request-id (inserted by SetRequestIdLayer)
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string();

    // Capture path + query without borrowing `request` across moves
    let uri = request.uri().clone();
    let path = uri
        .path_and_query()
        .map_or_else(|| uri.path().to_string(), |pq| pq.as_str().to_string());

    // Decide if we log the request body (avoid multipart)
    let request_ct = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Split parts/body
    let (request_parts, request_body) = request.into_parts();
    let request = if request_ct.starts_with("multipart/") {
        Request::from_parts(request_parts, request_body)
    } else {
        match axum::body::to_bytes(request_body, 64 * 1024).await {
            Ok(bytes) => {
                let preview = if bytes.len() > 16 * 1024 {
                    format!(
                        "{}… [truncated]",
                        String::from_utf8_lossy(&bytes[..16 * 1024])
                    )
                } else {
                    String::from_utf8_lossy(&bytes).to_string()
                };

                tracing::debug!(
                    request_id = %request_id,
                    path = %path,
                    request_body = %preview,
                    "request body"
                );

                Request::from_parts(request_parts, Body::from(bytes))
            }
            Err(e) => {
                tracing::warn!(
                    request_id = %request_id,
                    path = %path,
                    error = %e,
                    "failed reading request body"
                );
                Request::from_parts(request_parts, Body::empty())
            }
        }
    };

    // Call handler
    let response: Response<Body> = next.run(request).await;

    // Response body logging decision
    let response_content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let (response_parts, response_body) = response.into_parts();

    if !response_content_type.starts_with("image/")
        && !response_content_type.starts_with("application/octet-stream")
    {
        match axum::body::to_bytes(response_body, 64 * 1024).await {
            Ok(bytes) => {
                let preview = if bytes.len() > 16 * 1024 {
                    format!(
                        "{}… [truncated]",
                        String::from_utf8_lossy(&bytes[..16 * 1024])
                    )
                } else {
                    String::from_utf8_lossy(&bytes).to_string()
                };

                tracing::debug!(
                    request_id = %request_id,
                    path = %path,
                    response_body = %preview,
                    "response body"
                );

                Response::from_parts(response_parts, Body::from(bytes))
            }
            Err(e) => {
                tracing::warn!(
                    request_id = %request_id,
                    path = %path,
                    error = %e,
                    "failed reading response body"
                );
                Response::from_parts(response_parts, Body::empty())
            }
        }
    } else {
        Response::from_parts(response_parts, response_body)
    }
}
