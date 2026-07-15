use std::convert::Infallible;

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, request::Parts},
};

const HX_REQUEST: &str = "hx-request";

/// Indicates whether the current request was issued by htmx.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HxRequest(bool);

impl HxRequest {
    /// Returns `true` only when `HX-Request` has the value `true`.
    #[must_use]
    pub const fn is_htmx(self) -> bool {
        self.0
    }

    /// Detects an htmx request from HTTP headers.
    #[must_use]
    pub fn from_headers(headers: &HeaderMap) -> Self {
        Self(
            headers
                .get(HX_REQUEST)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.eq_ignore_ascii_case("true")),
        )
    }
}

impl<S> FromRequestParts<S> for HxRequest
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    /// Extracts htmx request metadata without rejecting malformed headers.
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self::from_headers(&parts.headers))
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::HxRequest;

    #[test]
    fn recognizes_only_a_true_header_value() {
        let mut headers = HeaderMap::new();
        assert!(!HxRequest::from_headers(&headers).is_htmx());

        headers.insert("hx-request", HeaderValue::from_static("true"));
        assert!(HxRequest::from_headers(&headers).is_htmx());

        headers.insert("hx-request", HeaderValue::from_static("TRUE"));
        assert!(HxRequest::from_headers(&headers).is_htmx());

        headers.insert("hx-request", HeaderValue::from_static("false"));
        assert!(!HxRequest::from_headers(&headers).is_htmx());

        headers.insert(
            "hx-request",
            HeaderValue::from_bytes(&[0xff]).expect("opaque header value"),
        );
        assert!(!HxRequest::from_headers(&headers).is_htmx());
    }
}
