use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Request, Response, Uri, header};
use axum::middleware::Next;

const BODY_READ_LIMIT: usize = 64 * 1024; // max bytes we attempt to buffer
const BODY_PREVIEW_LIMIT: usize = 16 * 1024; // max bytes we print

pub async fn log_payloads(request: Request<Body>, next: Next) -> Response<Body> {
    let request_id = get_request_id(request.headers());
    let path = get_path(request.uri());

    let request = maybe_log_request_body(request, &request_id, &path).await;

    let response = next.run(request).await;

    maybe_log_response_body(response, &request_id, &path).await
}

/* =========================
 * Request helpers
 * ========================= */

async fn maybe_log_request_body(
    request: Request<Body>,
    request_id: &str,
    path: &str,
) -> Request<Body> {
    let request_ct = content_type(request.headers());
    let (parts, body) = request.into_parts();

    let len = content_len(&parts.headers);

    // Skip multipart bodies entirely
    if is_multipart(&request_ct) {
        return Request::from_parts(parts, body);
    }

    // Skip large bodies so we don't consume them & accidentally drop them on error
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
            // same behavior as your current code
            Request::from_parts(parts, Body::empty())
        }
    }
}

/* =========================
 * Response helpers
 * ========================= */

async fn maybe_log_response_body(
    response: Response<Body>,
    request_id: &str,
    path: &str,
) -> Response<Body> {
    let response_ct = content_type(response.headers());
    let (parts, body) = response.into_parts();

    // Skip likely-binary responses
    if is_likely_binary(&response_ct) {
        return Response::from_parts(parts, body);
    }

    let len = content_len(&parts.headers);

    // Same safety as request side: don't buffer huge bodies
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

/* =========================
 * Shared utility helpers
 * ========================= */

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
