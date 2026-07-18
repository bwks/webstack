use std::time::Instant;

use axum::{
    extract::{MatchedPath, Request},
    http::{
        HeaderValue,
        header::{
            CONTENT_SECURITY_POLICY, HeaderName, REFERRER_POLICY, STRICT_TRANSPORT_SECURITY,
            X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
        },
    },
    middleware::Next,
    response::Response,
};
use tracing::Instrument;
use uuid::Uuid;

const CSP: &str = "default-src 'self'; base-uri 'self'; connect-src 'self'; font-src 'self'; form-action 'self'; frame-ancestors 'none'; img-src 'self' data:; object-src 'none'; script-src 'self'; style-src 'self'";
const HSTS: &str = "max-age=31536000";
const PERMISSIONS_POLICY: HeaderName = HeaderName::from_static("permissions-policy");
const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Assigns a trusted request ID, records request telemetry, and secures the response.
pub(crate) async fn middleware(tls_active: bool, mut request: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let request_id_value =
        HeaderValue::from_str(&request_id).expect("UUID is a valid header value");
    request
        .headers_mut()
        .insert(REQUEST_ID.clone(), request_id_value.clone());
    let method = request.method().clone();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("<unmatched>", MatchedPath::as_str);
    let started = Instant::now();
    let span = tracing::info_span!(
        "http_request",
        request_id = %request_id,
        method = %method,
        route = %route
    );
    let mut response = next.run(request).instrument(span.clone()).await;
    let latency_ms = started.elapsed().as_millis();
    tracing::info!(
        parent: &span,
        status = response.status().as_u16(),
        latency_ms,
        "request completed"
    );
    response.headers_mut().insert(REQUEST_ID, request_id_value);
    apply_security_headers(&mut response, tls_active);
    response
}

/// Adds the fixed browser security policy to one response.
fn apply_security_headers(response: &mut Response, tls_active: bool) {
    let headers = response.headers_mut();
    headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(
        PERMISSIONS_POLICY,
        HeaderValue::from_static("camera=(), geolocation=(), microphone=()"),
    );
    if tls_active {
        headers.insert(STRICT_TRANSPORT_SECURITY, HeaderValue::from_static(HSTS));
    } else {
        headers.remove(STRICT_TRANSPORT_SECURITY);
    }
}
