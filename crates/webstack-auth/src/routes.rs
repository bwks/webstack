use std::{
    convert::Infallible,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use crate::{AuthBackend, Credentials, CsrfToken, LoginThrottle, hash_password};
use axum::{
    Form, Router,
    extract::{ConnectInfo, Extension, FromRequestParts, Request},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{MethodRouter, post},
};
use axum_login::AuthSession;
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use tower_cookies::Cookies;
use tower_sessions::Session;

const ABSOLUTE_EXPIRY_KEY: &str = "webstack.auth.expires_at";
const FLASH_KEY: &str = "webstack.auth.message";

/// A safe authentication message consumed by an application-owned page.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMessage {
    InvalidCredentials,
    PasswordExpired,
    PasswordMismatch,
    PasswordLength,
    PasswordUnchanged,
    PasswordChanged,
}

/// Data supplied to an application-owned login page handler.
pub struct LoginPageContext {
    pub csrf_token: CsrfToken,
    pub message: Option<AuthMessage>,
}

impl<S> FromRequestParts<S> for LoginPageContext
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    /// Loads login-page CSRF and one-time message state.
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let csrf_token = CsrfToken::from_request_parts(parts, state)
            .await
            .expect("infallible CSRF token");
        let session = Session::from_request_parts(parts, state)
            .await
            .expect("session layer");
        let message = session
            .remove::<AuthMessage>(FLASH_KEY)
            .await
            .ok()
            .flatten();
        Ok(Self {
            csrf_token,
            message,
        })
    }
}

/// Data supplied to an application-owned password-change page handler.
pub struct PasswordChangePageContext {
    pub csrf_token: CsrfToken,
    pub message: Option<AuthMessage>,
}

impl<S> FromRequestParts<S> for PasswordChangePageContext
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    /// Loads password-page CSRF and one-time message state.
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let login = LoginPageContext::from_request_parts(parts, state)
            .await
            .expect("infallible login context");
        Ok(Self {
            csrf_token: login.csrf_token,
            message: login.message,
        })
    }
}

struct ClientAddress(IpAddr);

impl<S> FromRequestParts<S> for ClientAddress
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    /// Reads Axum's connection metadata or uses an unspecified address in tests.
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let address = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |ConnectInfo(address)| {
                address.ip()
            });
        Ok(Self(address))
    }
}

/// Runtime values shared by framework-owned authentication handlers.
#[derive(Clone, Debug)]
pub struct AuthRuntime {
    pub backend: AuthBackend,
    pub session_ttl_hours: u64,
    pub password_ttl_days: u16,
    pub throttle: LoginThrottle,
}

/// Role required by one protected route group.
#[derive(Clone, Debug)]
pub struct RequiredRole(String);

impl RequiredRole {
    /// Creates a route role requirement.
    #[must_use]
    pub fn new(role: impl Into<String>) -> Self {
        Self(role.into())
    }

    /// Returns the configured role name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
    #[serde(rename = "_csrf")]
    csrf: String,
}

#[derive(Deserialize)]
struct PasswordForm {
    current_password: String,
    new_password: String,
    confirm_password: String,
    #[serde(rename = "_csrf")]
    csrf: String,
}

#[derive(Deserialize)]
struct LogoutForm {
    #[serde(rename = "_csrf")]
    csrf: String,
}

/// Builds framework authentication routes around application-owned GET pages.
pub fn auth_router<S>(login_page: MethodRouter<S>, password_page: MethodRouter<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/login", login_page.merge(post(login)))
        .route("/logout", post(logout))
        .route(
            "/change-password",
            password_page.merge(post(change_password)),
        )
}

/// Authenticates one login form and creates an absolute-lifetime session.
async fn login(
    ClientAddress(client_ip): ClientAddress,
    Extension(runtime): Extension<AuthRuntime>,
    mut auth_session: AuthSession<AuthBackend>,
    session: Session,
    cookies: Cookies,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    if !valid_form_csrf(&session, &cookies, &form.csrf).await {
        return StatusCode::FORBIDDEN.into_response();
    }
    let username = form.username.trim().to_ascii_lowercase();
    let key = format!("{client_ip}:{username}");
    if let Some(remaining) = runtime.throttle.remaining(&key).await {
        return retry_response(remaining);
    }
    let credentials = Credentials {
        username,
        password: form.password,
    };
    match auth_session.authenticate(credentials).await {
        Ok(Some(user)) => {
            runtime.throttle.clear(&key).await;
            if auth_session.login(&user).await.is_err() {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
            let expires = OffsetDateTime::now_utc()
                .saturating_add(Duration::hours(
                    i64::try_from(runtime.session_ttl_hours).unwrap_or(i64::MAX),
                ))
                .unix_timestamp();
            let _result = session.insert(ABSOLUTE_EXPIRY_KEY, expires).await;
            if user.password_expired() {
                redirect_response(&headers, "/change-password")
            } else {
                redirect_response(&headers, "/")
            }
        }
        Ok(None) => {
            runtime.throttle.record_failure(key).await;
            let _result = session
                .insert(FLASH_KEY, AuthMessage::InvalidCredentials)
                .await;
            redirect_response(&headers, "/login")
        }
        Err(error) => {
            tracing::error!(%error, "authentication backend failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Ends the current authenticated session.
async fn logout(
    mut auth_session: AuthSession<AuthBackend>,
    session: Session,
    cookies: Cookies,
    headers: HeaderMap,
    Form(form): Form<LogoutForm>,
) -> Response {
    if !valid_form_csrf(&session, &cookies, &form.csrf).await {
        return StatusCode::FORBIDDEN.into_response();
    }
    if auth_session.logout().await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    redirect_response(&headers, "/login")
}

/// Requires a live authenticated account with a non-expired password.
pub async fn require_authenticated(
    mut auth_session: AuthSession<AuthBackend>,
    session: Session,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();
    let Some(user) = auth_session.user.as_ref() else {
        return auth_redirect(headers, "/login");
    };
    let absolute_expiry = session.get::<i64>(ABSOLUTE_EXPIRY_KEY).await.ok().flatten();
    if absolute_expiry.is_none_or(|expires| expires <= OffsetDateTime::now_utc().unix_timestamp()) {
        let _result = auth_session.logout().await;
        return auth_redirect(headers, "/login");
    }
    if user.password_expired() {
        let _result = session
            .insert(FLASH_KEY, AuthMessage::PasswordExpired)
            .await;
        return redirect_response(headers, "/change-password");
    }
    next.run(request).await
}

/// Requires an authenticated account carrying the supplied role name.
pub async fn require_role(
    Extension(required_role): Extension<RequiredRole>,
    auth_session: AuthSession<AuthBackend>,
    request: Request,
    next: Next,
) -> Response {
    let Some(user) = auth_session.user.as_ref() else {
        return auth_redirect(request.headers(), "/login");
    };
    if user.password_expired() {
        return redirect_response(request.headers(), "/change-password");
    }
    if !user.has_role(required_role.as_str()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(request).await
}

/// Validates and replaces an authenticated user's password.
async fn change_password(
    Extension(runtime): Extension<AuthRuntime>,
    mut auth_session: AuthSession<AuthBackend>,
    session: Session,
    cookies: Cookies,
    headers: HeaderMap,
    Form(form): Form<PasswordForm>,
) -> Response {
    if !valid_form_csrf(&session, &cookies, &form.csrf).await {
        return StatusCode::FORBIDDEN.into_response();
    }
    let absolute_expiry = session.get::<i64>(ABSOLUTE_EXPIRY_KEY).await.ok().flatten();
    if absolute_expiry.is_none_or(|expires| expires <= OffsetDateTime::now_utc().unix_timestamp()) {
        let _result = auth_session.logout().await;
        return auth_redirect(&headers, "/login");
    }
    let Some(user) = auth_session.user.clone() else {
        return auth_redirect(&headers, "/login");
    };
    let message = if form.new_password != form.confirm_password {
        Some(AuthMessage::PasswordMismatch)
    } else if !(12..=128).contains(&form.new_password.chars().count()) {
        Some(AuthMessage::PasswordLength)
    } else if form.new_password == form.current_password {
        Some(AuthMessage::PasswordUnchanged)
    } else {
        None
    };
    if let Some(message) = message {
        let _result = session.insert(FLASH_KEY, message).await;
        return redirect_response(&headers, "/change-password");
    }
    let current = Credentials {
        username: user.username().to_owned(),
        password: form.current_password,
    };
    match auth_session.authenticate(current).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            let _result = session
                .insert(FLASH_KEY, AuthMessage::InvalidCredentials)
                .await;
            return redirect_response(&headers, "/change-password");
        }
        Err(error) => {
            tracing::error!(%error, "password verification backend failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    let Ok(password_hash) = hash_password(&form.new_password).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if runtime
        .backend
        .replace_password(user.username(), &password_hash, runtime.password_ttl_days)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let _result = auth_session.logout().await;
    let _result = session
        .insert(FLASH_KEY, AuthMessage::PasswordChanged)
        .await;
    redirect_response(&headers, "/login")
}

/// Checks the hidden form token against session and cookie state.
async fn valid_form_csrf(session: &Session, cookies: &Cookies, submitted: &str) -> bool {
    let session_token = session.get::<String>("webstack.csrf").await.ok().flatten();
    let cookie_token = cookies
        .get("webstack.csrf")
        .map(|cookie| cookie.value().to_owned());
    session_token.as_deref() == Some(submitted) && session_token == cookie_token
}

/// Builds a normal redirect or htmx client redirect.
fn redirect_response(headers: &HeaderMap, target: &'static str) -> Response {
    if headers.get("hx-request").is_some() {
        let mut response = StatusCode::OK.into_response();
        response
            .headers_mut()
            .insert("hx-redirect", HeaderValue::from_static(target));
        response
    } else {
        (StatusCode::SEE_OTHER, [(header::LOCATION, target)]).into_response()
    }
}

/// Builds an authentication redirect while preserving unauthorized semantics for htmx.
fn auth_redirect(headers: &HeaderMap, target: &'static str) -> Response {
    if headers.get("hx-request").is_some() {
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        response
            .headers_mut()
            .insert("hx-redirect", HeaderValue::from_static(target));
        response
    } else {
        (StatusCode::SEE_OTHER, [(header::LOCATION, target)]).into_response()
    }
}

/// Builds a rate-limit response with a bounded retry delay.
fn retry_response(remaining: std::time::Duration) -> Response {
    let seconds = remaining.as_secs().max(1).to_string();
    let mut response = StatusCode::TOO_MANY_REQUESTS.into_response();
    if let Ok(value) = HeaderValue::from_str(&seconds) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}
