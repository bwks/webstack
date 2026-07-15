use std::convert::Infallible;

use axum::{
    body::{Body, to_bytes},
    extract::{FromRequestParts, Request},
    http::{Method, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use tower_cookies::{Cookie, Cookies, cookie::SameSite};
use tower_sessions::Session;

const CSRF_KEY: &str = "webstack.csrf";
const CSRF_COOKIE: &str = "webstack.csrf";
const CSRF_HEADER: &str = "x-csrf-token";
const MAX_FORM_BODY_BYTES: usize = 64 * 1024;

/// A session-bound token for generated page templates and unsafe requests.
#[derive(Clone, Debug)]
pub struct CsrfToken(String);

impl CsrfToken {
    /// Returns the token text for forms and `hx-headers`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<S> FromRequestParts<S> for CsrfToken
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    /// Loads or creates the request's session-bound CSRF token.
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .expect("session layer");
        let cookies = Cookies::from_request_parts(parts, state)
            .await
            .expect("cookie layer");
        let token = if let Ok(Some(token)) = session.get::<String>(CSRF_KEY).await {
            token
        } else {
            let mut bytes = [0_u8; 32];
            OsRng.fill_bytes(&mut bytes);
            let token = URL_SAFE_NO_PAD.encode(bytes);
            let _result = session.insert(CSRF_KEY, token.clone()).await;
            token
        };
        if cookies
            .get(CSRF_COOKIE)
            .is_none_or(|cookie| cookie.value() != token)
        {
            let cookie = Cookie::build((CSRF_COOKIE, token.clone()))
                .path("/")
                .http_only(true)
                .same_site(SameSite::Lax)
                .build();
            cookies.add(cookie);
        }
        Ok(Self(token))
    }
}

/// Rejects unsafe requests without matching session, cookie, and request tokens.
///
/// htmx requests normally send the token in `X-CSRF-Token`. Regular HTML form
/// submissions may provide the same value in a `_csrf` form field so generated
/// applications retain progressive enhancement without JavaScript.
pub async fn csrf_middleware(
    session: Session,
    cookies: Cookies,
    request: Request,
    next: Next,
) -> Response {
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        return next.run(request).await;
    }
    let session_token = session.get::<String>(CSRF_KEY).await.ok().flatten();
    let cookie_token = cookies
        .get(CSRF_COOKIE)
        .map(|cookie| cookie.value().to_owned());
    let header_token = request
        .headers()
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if session_token.is_some() && session_token == cookie_token && session_token == header_token {
        next.run(request).await
    } else if session_token.is_some()
        && session_token == cookie_token
        && request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("application/x-www-form-urlencoded"))
    {
        let (parts, body) = request.into_parts();
        let Ok(bytes) = to_bytes(body, MAX_FORM_BODY_BYTES).await else {
            return StatusCode::FORBIDDEN.into_response();
        };
        let form_token = form_urlencoded::parse(&bytes)
            .find(|(name, _value)| name == "_csrf")
            .map(|(_name, value)| value.into_owned());
        let request = Request::from_parts(parts, Body::from(bytes));
        if session_token == form_token {
            next.run(request).await
        } else {
            StatusCode::FORBIDDEN.into_response()
        }
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}
