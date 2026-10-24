//! Embedded static asset responses.

use std::fmt::Write as _;

use axum::{
    extract::{Path, Query, rejection::QueryRejection},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;
use serde::Deserialize;

/// Cache policy for a request whose `v` parameter names the current content.
const VERSIONED_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";
/// Cache policy for unversioned or outdated URLs, which must revalidate.
const REVALIDATE_CACHE_CONTROL: &str = "no-cache";
/// Hexadecimal characters of the content hash used as the URL version.
const VERSION_LENGTH: usize = 16;

/// Query parameters accepted by the static asset route.
#[derive(Debug, Default, Deserialize)]
pub struct AssetQuery {
    v: Option<String>,
}

/// Returns the versioned URL for one file in an application-owned
/// [`RustEmbed`] collection.
///
/// The `v` parameter is derived from the file's content hash, so a changed
/// file gets a new URL and browsers can cache each version indefinitely.
/// Unknown paths return the unversioned URL so the route reports `404`.
#[must_use]
pub fn url<A>(path: &str) -> String
where
    A: RustEmbed,
{
    let path = path.trim_start_matches('/');
    match A::get(path) {
        Some(file) => format!(
            "/static/{path}?v={}",
            version(&hex(file.metadata.sha256_hash()))
        ),
        None => format!("/static/{path}"),
    }
}

/// Serves one file from an application-owned [`RustEmbed`] collection.
///
/// Requests carrying the current content version are cacheable as immutable;
/// every other request must revalidate with the content-hash `ETag`.
pub async fn serve<A>(
    Path(path): Path<String>,
    query: Result<Query<AssetQuery>, QueryRejection>,
    request_headers: HeaderMap,
) -> Response
where
    A: RustEmbed,
{
    let Some(file) = A::get(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let hash = hex(file.metadata.sha256_hash());
    let cache_control = if query
        .ok()
        .and_then(|Query(query)| query.v)
        .is_some_and(|requested| requested == version(&hash))
    {
        VERSIONED_CACHE_CONTROL
    } else {
        REVALIDATE_CACHE_CONTROL
    };
    let etag = format!("\"{hash}\"");
    if request_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|value| value.trim() == etag))
    {
        return (
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag),
                (header::CACHE_CONTROL, cache_control.to_owned()),
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
            (header::CACHE_CONTROL, cache_control.to_owned()),
        ],
        file.data,
    )
        .into_response()
}

/// Shortens a hexadecimal content hash to the URL version token.
fn version(hash: &str) -> &str {
    &hash[..VERSION_LENGTH]
}

/// Encodes a SHA-256 byte array as lowercase hexadecimal.
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
        http::{Request, Response, StatusCode, header},
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

    async fn get_with(uri: &str, if_none_match: Option<&str>) -> Response<Body> {
        let mut request = Request::get(uri);
        if let Some(etag) = if_none_match {
            request = request.header(header::IF_NONE_MATCH, etag);
        }
        router()
            .oneshot(request.body(Body::empty()).expect("request"))
            .await
            .expect("response")
    }

    #[test]
    fn urls_carry_the_content_version_of_known_assets() {
        let url = super::url::<Assets>("example.css");
        let version = url
            .strip_prefix("/static/example.css?v=")
            .expect("versioned URL");
        assert_eq!(version.len(), super::VERSION_LENGTH);
        assert!(version.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(super::url::<Assets>("/example.css"), url);
        assert_eq!(super::url::<Assets>("missing.js"), "/static/missing.js");
    }

    #[tokio::test]
    async fn serves_content_metadata_and_conditional_requests() {
        let response = get_with("/static/example.css", None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "text/css");
        let etag = response.headers()[header::ETAG]
            .to_str()
            .expect("ETag")
            .to_owned();
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body"),
            "body { color: rebeccapurple; }\n"
        );

        let response = get_with("/static/example.css", Some(&etag)).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(response.headers()[header::ETAG], etag.as_str());
    }

    #[tokio::test]
    async fn only_current_versions_are_cached_as_immutable() {
        let current = super::url::<Assets>("example.css");
        let response = get_with(&current, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            super::VERSIONED_CACHE_CONTROL
        );

        for uri in [
            "/static/example.css",
            "/static/example.css?v=0000000000000000",
            "/static/example.css?other=1",
            "/static/example.css?v=a&v=b",
        ] {
            let response = get_with(uri, None).await;
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                super::REVALIDATE_CACHE_CONTROL,
                "{uri}"
            );
        }
    }

    #[tokio::test]
    async fn revalidation_keeps_the_cache_policy_of_the_request() {
        let etag = get_with("/static/example.css", None).await.headers()[header::ETAG]
            .to_str()
            .expect("ETag")
            .to_owned();
        let current = super::url::<Assets>("example.css");

        let response = get_with(&current, Some(&etag)).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            super::VERSIONED_CACHE_CONTROL
        );

        let response = get_with("/static/example.css", Some(&etag)).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            super::REVALIDATE_CACHE_CONTROL
        );
    }

    #[tokio::test]
    async fn missing_assets_return_not_found() {
        let response = get_with("/static/missing.js", None).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
