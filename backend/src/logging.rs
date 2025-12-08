use crate::config::Config;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Request, Response, Uri, header};
use axum::middleware::Next;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    EnvFilter, filter::Directive, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};

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

fn build_filter(config: &Config) -> EnvFilter {
    let mut filter = EnvFilter::new(config.log_filter());

    // Hard-disable ultra-noisy HTML parsing internals forever.
    // These targets are commonly responsible for "processing TagToken..." spam.
    for d in [
        "html5ever=off",
        "markup5ever=off",
        "selectors=off",
        "tendril=off",
        "string_cache=off",
    ] {
        if let Ok(dir) = d.parse::<Directive>() {
            filter = filter.add_directive(dir);
        }
    }

    filter
}

#[must_use]
pub fn init_logging(config: &Config) -> LogGuards {
    let filter = build_filter(config);

    // Stdout layer (pretty enough, ANSI enabled)
    let stdout_layer = fmt::layer()
        .with_target(false)
        .with_ansi(true)
        .compact()
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
/// 4xx/5xx -> ERROR
///
/// Includes query string.
pub async fn access_log(request: Request<Body>, next: Next) -> Response<Body> {
    let method = request.method().clone();

    let uri = request.uri().clone();
    let path = uri
        .path_and_query()
        .map_or_else(|| uri.path().to_string(), |pq| pq.as_str().to_string());

    let res = next.run(request).await;
    let status = res.status().as_u16();

    let msg = format!("{method:<6} {path:<40} {status}");

    if (400..=599).contains(&status) {
        tracing::error!("{msg}");
    } else {
        tracing::info!("{msg}");
    }

    res
}

/* =========================
 * Payload logging (split)
 * ========================= */

const BODY_READ_LIMIT: usize = 64 * 1024;
const BODY_PREVIEW_LIMIT: usize = 16 * 1024;

/// Logs request & response bodies (dev-friendly).
/// Skips multipart requests and likely-binary responses, truncates previews.
/// Includes request-id for correlation.
///
/// These logs are DEBUG so default verbosity stays clean.
pub async fn log_payloads(request: Request<Body>, next: Next) -> Response<Body> {
    let request_id = get_request_id(request.headers());
    let path = get_path(request.uri());

    let request = maybe_log_request_body(request, &request_id, &path).await;

    let response = next.run(request).await;

    maybe_log_response_body(response, &request_id, &path).await
}

/* ---------- request helpers ---------- */

async fn maybe_log_request_body(
    request: Request<Body>,
    request_id: &str,
    path: &str,
) -> Request<Body> {
    let request_ct = content_type(request.headers());
    let (parts, body) = request.into_parts();
    let len = content_len(&parts.headers);

    if is_multipart(&request_ct) {
        return Request::from_parts(parts, body);
    }

    // Avoid consuming large bodies.
    if len > BODY_READ_LIMIT as u64 {
        return Request::from_parts(parts, body);
    }

    match read_body_with_preview(body).await {
        Ok((bytes, preview)) => {
            tracing::debug!(
                request_id = %request_id,
                path = %path,
                request_body = %preview,
                "request body"
            );
            Request::from_parts(parts, Body::from(bytes))
        }
        Err(e) => {
            tracing::warn!(
                request_id = %request_id,
                path = %path,
                error = %e,
                "failed reading request body"
            );
            Request::from_parts(parts, Body::empty())
        }
    }
}

/* ---------- response helpers ---------- */

async fn maybe_log_response_body(
    response: Response<Body>,
    request_id: &str,
    path: &str,
) -> Response<Body> {
    let response_ct = content_type(response.headers());
    let (parts, body) = response.into_parts();
    let len = content_len(&parts.headers);

    if is_likely_binary(&response_ct) {
        return Response::from_parts(parts, body);
    }

    // Avoid consuming large bodies.
    if len > BODY_READ_LIMIT as u64 {
        return Response::from_parts(parts, body);
    }

    match read_body_with_preview(body).await {
        Ok((bytes, preview)) => {
            tracing::debug!(
                request_id = %request_id,
                path = %path,
                response_body = %preview,
                "response body"
            );
            Response::from_parts(parts, Body::from(bytes))
        }
        Err(e) => {
            tracing::warn!(
                request_id = %request_id,
                path = %path,
                error = %e,
                "failed reading response body"
            );
            Response::from_parts(parts, Body::empty())
        }
    }
}

/* ---------- shared utils ---------- */

fn get_request_id(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string()
}

fn get_path(uri: &Uri) -> String {
    uri.path_and_query()
        .map_or_else(|| uri.path().to_string(), |pq| pq.as_str().to_string())
}

fn content_type(headers: &HeaderMap) -> String {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

fn content_len(headers: &HeaderMap) -> u64 {
    headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

fn is_multipart(ct: &str) -> bool {
    ct.starts_with("multipart/")
}

fn is_likely_binary(ct: &str) -> bool {
    ct.starts_with("image/") || ct.starts_with("application/octet-stream")
}

async fn read_body_with_preview(body: Body) -> Result<(Bytes, String), axum::Error> {
    let bytes = axum::body::to_bytes(body, BODY_READ_LIMIT).await?;
    let preview = make_preview(&bytes);
    Ok((bytes, preview))
}

fn make_preview(bytes: &Bytes) -> String {
    if bytes.len() > BODY_PREVIEW_LIMIT {
        format!(
            "{}… [truncated]",
            String::from_utf8_lossy(&bytes[..BODY_PREVIEW_LIMIT])
        )
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}
