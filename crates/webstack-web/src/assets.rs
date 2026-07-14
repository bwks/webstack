//! Embedded static asset responses.

use std::fmt::Write as _;

use axum::{
    extract::Path,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

const CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

/// Serves one file from an application-owned [`RustEmbed`] collection.
pub async fn serve<A>(Path(path): Path<String>, request_headers: HeaderMap) -> Response
where
    A: RustEmbed,
{
    let Some(file) = A::get(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let etag = format!("\"{}\"", hex(file.metadata.sha256_hash()));
    if request_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|value| value.trim() == etag))
    {
        return (
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag),
                (header::CACHE_CONTROL, CACHE_CONTROL.to_owned()),
            ],
        )
            .into_response();
    }

    let content_type = mime_guess::from_path(&path).first_or_octet_stream();
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::ETAG, etag),
            (header::CACHE_CONTROL, CACHE_CONTROL.to_owned()),
        ],
        file.data,
    )
        .into_response()
}

fn hex(bytes: [u8; 32]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(value, "{byte:02x}");
    }
    value
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
        routing::get,
    };
    use rust_embed::RustEmbed;
    use tower::ServiceExt;

    #[derive(RustEmbed)]
    #[folder = "tests/fixtures/assets"]
    struct Assets;

    fn router() -> Router {
        Router::new().route("/static/{*path}", get(super::serve::<Assets>))
    }

    #[tokio::test]
    async fn serves_content_metadata_and_conditional_requests() {
        let response = router()
            .oneshot(
                Request::get("/static/example.css")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "text/css");
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            super::CACHE_CONTROL
        );
        let etag = response.headers()[header::ETAG].clone();
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body"),
            "body { color: rebeccapurple; }\n"
        );

        let response = router()
            .oneshot(
                Request::get("/static/example.css")
                    .header(header::IF_NONE_MATCH, etag)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn missing_assets_return_not_found() {
        let response = router()
            .oneshot(
                Request::get("/static/missing.js")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
