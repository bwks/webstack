use std::{convert::Infallible, path::Path, sync::Arc};

use axum::{
    Extension, Json, Router,
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{MethodRouter, get},
};
use axum_login::AuthManagerLayerBuilder;
use serde::Serialize;
use thiserror::Error;
use tower_sessions::{ExpiredDeletion, Expiry, SessionManagerLayer};
use webstack_auth::{
    AuthBackend, AuthError, AuthRuntime, LoginThrottle, RequiredRole, SurrealSessionStore,
    auth_router, bootstrap_admin, csrf_middleware, require_authenticated, require_role,
};
use webstack_core::{
    config::{Config, ConfigError, TlsMode},
    observability::{self, ObservabilityError},
};
use webstack_db::{Database, DatabaseError};

use crate::{
    error::{ErrorMarker, ErrorRenderer, ErrorView, render_error},
    htmx::HxRequest,
    tls::TlsError,
};

use rust_embed::RustEmbed;

const HEALTH_PATH: &str = "/healthz";
const STATIC_PATH: &str = "/static";

/// Entry point for composing one generated application's HTTP runtime.
pub struct Application;

impl Application {
    /// Starts composing a Webstack application.
    #[must_use]
    pub fn builder() -> ApplicationBuilder {
        ApplicationBuilder {
            router: Router::new(),
            route_count: 0,
            assets_registered: false,
            auth_pages_registered: false,
            error_renderer: None,
        }
    }
}

/// Fluent application-owned route builder.
pub struct ApplicationBuilder {
    router: Router<AppState>,
    route_count: usize,
    assets_registered: bool,
    auth_pages_registered: bool,
    error_renderer: Option<ErrorRenderer>,
}

impl ApplicationBuilder {
    /// Adds an application-owned route.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::ReservedRoute`] for `/healthz`.
    pub fn route(
        mut self,
        path: &str,
        method_router: MethodRouter<AppState>,
    ) -> Result<Self, ApplicationError> {
        if path == HEALTH_PATH
            || path == STATIC_PATH
            || path.starts_with("/static/")
            || matches!(path, "/login" | "/logout" | "/change-password")
        {
            return Err(ApplicationError::ReservedRoute(path.to_owned()));
        }
        self.router = self.router.route(
            path,
            method_router.layer(middleware::from_fn(csrf_middleware)),
        );
        self.route_count += 1;
        Ok(self)
    }

    /// Registers application-owned login and password-change pages.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::AuthPagesAlreadyRegistered`] when called twice.
    pub fn auth_pages(
        mut self,
        login_page: MethodRouter<AppState>,
        password_page: MethodRouter<AppState>,
    ) -> Result<Self, ApplicationError> {
        if self.auth_pages_registered {
            return Err(ApplicationError::AuthPagesAlreadyRegistered);
        }
        self.router = self.router.merge(auth_router(login_page, password_page));
        self.auth_pages_registered = true;
        self.route_count += 2;
        Ok(self)
    }

    /// Adds a route requiring a live authenticated account.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::ReservedRoute`] for framework-owned paths.
    pub fn authenticated_route(
        self,
        path: &str,
        method_router: MethodRouter<AppState>,
    ) -> Result<Self, ApplicationError> {
        self.route(
            path,
            method_router.layer(middleware::from_fn(require_authenticated)),
        )
    }

    /// Adds a route requiring a live account with the supplied role.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::ReservedRoute`] or an invalid-role error.
    pub fn role_route(
        self,
        path: &str,
        role: &str,
        method_router: MethodRouter<AppState>,
    ) -> Result<Self, ApplicationError> {
        if !valid_role(role) {
            return Err(ApplicationError::InvalidRole(role.to_owned()));
        }
        self.authenticated_route(
            path,
            method_router
                .layer::<_, Infallible>(middleware::from_fn(require_role))
                .layer(Extension(RequiredRole::new(role))),
        )
    }

    /// Registers an application-owned embedded asset collection at `/static`.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::AssetsAlreadyRegistered`] when called twice.
    pub fn assets<A>(mut self) -> Result<Self, ApplicationError>
    where
        A: RustEmbed + Send + Sync + 'static,
    {
        if self.assets_registered {
            return Err(ApplicationError::AssetsAlreadyRegistered);
        }
        self.router = self
            .router
            .route("/static/{*path}", get(crate::assets::serve::<A>));
        self.assets_registered = true;
        Ok(self)
    }

    /// Registers application-owned full-page and htmx error rendering.
    ///
    /// Returning `None` from the renderer selects the safe framework fallback.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::ErrorRendererAlreadyRegistered`] when called twice.
    pub fn error_renderer<F>(mut self, renderer: F) -> Result<Self, ApplicationError>
    where
        F: Fn(&ErrorView) -> Option<String> + Send + Sync + 'static,
    {
        if self.error_renderer.is_some() {
            return Err(ApplicationError::ErrorRendererAlreadyRegistered);
        }
        self.error_renderer = Some(Arc::new(renderer));
        Ok(self)
    }

    /// Returns the number of application-owned routes for framework tests.
    #[doc(hidden)]
    #[must_use]
    pub const fn route_count(&self) -> usize {
        self.route_count
    }

    /// Loads configuration, initializes tracing, and serves until shutdown.
    ///
    /// # Errors
    ///
    /// Returns a typed startup, TLS, or server failure. Backups fail clearly
    /// until their implementation milestone is complete.
    pub async fn run(self) -> Result<(), ApplicationError> {
        let config = Config::load()?;
        validate_runtime_features(&config)?;
        observability::init(&config.observability)?;

        let database = webstack_db::connect(
            &config.database.data_dir,
            &config.database.namespace,
            &config.database.database,
        )
        .await?;
        webstack_db::migrate(&database, Path::new("./migrations")).await?;
        bootstrap_admin(&database, &config.auth, config.environment).await?;

        let cleanup = spawn_session_cleanup(database.clone());
        let config = Arc::new(config);
        let router = self.into_router(Arc::clone(&config), database);
        let result = crate::tls::serve(&config, router, shutdown_signal())
            .await
            .map_err(ApplicationError::Tls);
        cleanup.abort();
        result
    }

    /// Converts the builder into a stateful Axum router with framework routes.
    fn into_router(self, config: Arc<Config>, database: Database) -> Router {
        let error_renderer = self.error_renderer.clone();
        let store = SurrealSessionStore::new(database.clone());
        let session_ttl =
            time::Duration::hours(i64::try_from(config.auth.session_ttl_hours).unwrap_or(i64::MAX));
        let session_layer = SessionManagerLayer::new(store)
            .with_name("webstack.sid")
            .with_secure(config.tls.mode != TlsMode::Disabled)
            .with_expiry(Expiry::OnInactivity(session_ttl));
        let backend = AuthBackend::new(database.clone());
        let runtime = AuthRuntime {
            backend: backend.clone(),
            session_ttl_hours: config.auth.session_ttl_hours,
            password_ttl_days: config.auth.password_ttl_days,
            throttle: LoginThrottle::default(),
        };
        let auth_layer = AuthManagerLayerBuilder::new(backend, session_layer).build();
        self.router
            .route(HEALTH_PATH, get(health))
            .with_state(AppState { config, database })
            .layer(Extension(runtime))
            .layer(auth_layer)
            .layer(middleware::from_fn(move |request, next| {
                render_application_error(request, next, error_renderer.clone())
            }))
    }
}

/// Framework state available to application handlers through Axum `State`.
pub struct AppState {
    config: Arc<Config>,
    database: Database,
}

impl Clone for AppState {
    /// Clones the shared application state handles.
    fn clone(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            database: self.database.clone(),
        }
    }
}

impl AppState {
    /// Returns the validated Webstack configuration.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Returns the application's cheaply cloneable shared database handle.
    #[must_use]
    pub const fn database(&self) -> &Database {
        &self.database
    }
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

/// A typed application composition or runtime failure.
#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("route {0:?} is reserved by Webstack")]
    ReservedRoute(String),
    #[error("embedded assets have already been registered")]
    AssetsAlreadyRegistered,
    #[error("authentication pages have already been registered")]
    AuthPagesAlreadyRegistered,
    #[error("an application error renderer has already been registered")]
    ErrorRendererAlreadyRegistered,
    #[error("role {0:?} must be lowercase snake_case and begin with a letter")]
    InvalidRole(String),
    #[error("{0} support is not implemented yet; disable it in configuration")]
    UnsupportedFeature(&'static str),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Observability(#[from] ObservabilityError),
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Tls(#[from] TlsError),
}

/// Re-renders only responses explicitly marked by [`crate::AppError`].
async fn render_application_error(
    request: Request,
    next: Next,
    renderer: Option<ErrorRenderer>,
) -> Response {
    let is_htmx = HxRequest::from_headers(request.headers()).is_htmx();
    let response = next.run(request).await;
    let Some(marker) = response.extensions().get::<ErrorMarker>() else {
        return response;
    };
    let view = marker.0.with_htmx(is_htmx);
    render_error(&view, renderer.as_ref())
}

/// Probes the embedded database and reports application readiness.
async fn health(State(state): State<AppState>) -> Response {
    match state.database.query("RETURN true;").await {
        Ok(response) => match response.check() {
            Ok(_) => Json(Health { status: "ok" }).into_response(),
            Err(error) => unavailable_health(&error),
        },
        Err(error) => unavailable_health(&error),
    }
}

/// Logs a database health failure and returns a detail-free readiness response.
fn unavailable_health(error: &impl std::fmt::Display) -> Response {
    tracing::error!(%error, "database health probe failed");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(Health {
            status: "unavailable",
        }),
    )
        .into_response()
}

/// Rejects configured runtime features that belong to later milestones.
fn validate_runtime_features(config: &Config) -> Result<(), ApplicationError> {
    if config.backup.enabled {
        return Err(ApplicationError::UnsupportedFeature("backup"));
    }
    Ok(())
}

/// Checks an application role against the public lowercase snake-case contract.
fn valid_role(role: &str) -> bool {
    let mut characters = role.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|character| character.is_ascii_lowercase() || character == '_')
}

/// Starts the daily expired-session cleanup task owned by the application runtime.
fn spawn_session_cleanup(database: Database) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let store = SurrealSessionStore::new(database);
        let mut interval = tokio::time::interval(std::time::Duration::from_hours(24));
        loop {
            interval.tick().await;
            if let Err(error) = store.delete_expired().await {
                tracing::error!(%error, "expired session cleanup failed");
            }
        }
    })
}

/// Waits for Ctrl+C or the platform's termination signal.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "failed to listen for Ctrl+C");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => tracing::error!(%error, "failed to listen for SIGTERM"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::{Body, to_bytes},
        extract::State,
        http::{Request, StatusCode, header},
        routing::{get, post},
    };
    use surrealdb::{Surreal, engine::local::Mem};
    use tower::ServiceExt;
    use webstack_auth::{LoginPageContext, PasswordChangePageContext, bootstrap_admin};
    use webstack_core::config::{Config, Environment, TlsMode};
    use webstack_db::Database;

    use super::{AppState, Application, ApplicationError, validate_runtime_features};
    use crate::AppError;

    async fn test_database() -> Database {
        let database = Surreal::new::<Mem>(()).await.expect("in-memory database");
        database
            .use_ns("test")
            .use_db("test")
            .await
            .expect("test namespace");
        database
    }

    async fn auth_database() -> Database {
        let database = test_database().await;
        database
            .query(
                r"
                DEFINE TABLE _webstack_user SCHEMAFULL;
                DEFINE FIELD username ON _webstack_user TYPE string;
                DEFINE FIELD password_hash ON _webstack_user TYPE string;
                DEFINE FIELD roles ON _webstack_user TYPE array<string>;
                DEFINE FIELD disabled ON _webstack_user TYPE bool;
                DEFINE FIELD created_at ON _webstack_user TYPE int;
                DEFINE FIELD password_expires_at ON _webstack_user TYPE int;
                DEFINE TABLE _webstack_session SCHEMAFULL;
                DEFINE FIELD payload ON _webstack_session TYPE string;
                DEFINE FIELD expires_at ON _webstack_session TYPE int;
                ",
            )
            .await
            .expect("auth schema query")
            .check()
            .expect("auth schema");
        database
    }

    fn response_cookies(response: &axum::response::Response) -> String {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .collect::<Vec<_>>()
            .join("; ")
    }

    #[tokio::test]
    async fn health_is_minimal_json_and_webstack_state_is_available() {
        let router = Application::builder()
            .route(
                "/",
                get(|State(state): State<AppState>| async move {
                    state
                        .database()
                        .query("RETURN $port;")
                        .bind(("port", state.config().server.http_port))
                        .await
                        .expect("state database")
                        .take::<Option<u16>>(0)
                        .expect("port result")
                        .expect("port")
                        .to_string()
                }),
            )
            .expect("application route")
            .into_router(Arc::new(Config::default()), test_database().await);

        let response = router
            .clone()
            .oneshot(
                Request::get("/healthz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("health response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body"),
            "{\"status\":\"ok\"}"
        );

        let response = router
            .oneshot(Request::get("/").body(Body::empty()).expect("request"))
            .await
            .expect("root response");
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body"),
            "8080"
        );
    }

    #[tokio::test]
    async fn unavailable_database_health_is_detail_free() {
        let router = Application::builder().into_router(
            Arc::new(Config::default()),
            Surreal::<surrealdb::engine::local::Db>::init(),
        );
        let response = router
            .oneshot(
                Request::get("/healthz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("health response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body"),
            "{\"status\":\"unavailable\"}"
        );
    }

    #[tokio::test]
    async fn auth_pages_login_and_authenticated_routes_share_database_sessions() {
        let database = auth_database().await;
        let config = Config {
            environment: Environment::Development,
            ..Config::default()
        };
        bootstrap_admin(&database, &config.auth, config.environment)
            .await
            .expect("bootstrap user");
        let account_database = database.clone();
        let router =
            Application::builder()
                .auth_pages(
                    get(|context: LoginPageContext| async move {
                        context.csrf_token.as_str().to_owned()
                    }),
                    get(|context: PasswordChangePageContext| async move {
                        context.csrf_token.as_str().to_owned()
                    }),
                )
                .expect("auth pages")
                .authenticated_route("/account", get(|| async { "account" }))
                .expect("protected route")
                .role_route("/admin", "admin", get(|| async { "admin" }))
                .expect("role route")
                .route("/unsafe", post(|| async { "changed" }))
                .expect("unsafe route")
                .into_router(Arc::new(config), database);

        let login_page = router
            .clone()
            .oneshot(Request::get("/login").body(Body::empty()).expect("request"))
            .await
            .expect("login page");
        assert_eq!(login_page.status(), StatusCode::OK);
        let cookies = response_cookies(&login_page);
        let csrf = String::from_utf8(
            to_bytes(login_page.into_body(), usize::MAX)
                .await
                .expect("csrf body")
                .to_vec(),
        )
        .expect("csrf text");
        let form = format!("username=admin&password=changeme&_csrf={csrf}");
        let login = router
            .clone()
            .oneshot(
                Request::post("/login")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(header::COOKIE, &cookies)
                    .body(Body::from(form))
                    .expect("login request"),
            )
            .await
            .expect("login response");
        assert_eq!(login.status(), StatusCode::SEE_OTHER);
        assert_eq!(login.headers()[header::LOCATION], "/");
        let authenticated_cookies = response_cookies(&login);

        let unsafe_form = router
            .clone()
            .oneshot(
                Request::post("/unsafe")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(
                        header::COOKIE,
                        format!("{cookies}; {authenticated_cookies}"),
                    )
                    .body(Body::from(format!("_csrf={csrf}")))
                    .expect("unsafe form request"),
            )
            .await
            .expect("unsafe form response");
        assert_eq!(unsafe_form.status(), StatusCode::OK);

        let account = router
            .clone()
            .oneshot(
                Request::get("/account")
                    .header(header::COOKIE, authenticated_cookies.clone())
                    .body(Body::empty())
                    .expect("account request"),
            )
            .await
            .expect("account response");
        assert_eq!(account.status(), StatusCode::OK);

        account_database
            .query("UPDATE type::record('_webstack_user', 'admin') SET roles = ['user'];")
            .await
            .expect("role update")
            .check()
            .expect("role update check");
        let forbidden = router
            .oneshot(
                Request::get("/admin")
                    .header(header::COOKIE, authenticated_cookies)
                    .body(Body::empty())
                    .expect("admin request"),
            )
            .await
            .expect("admin response");
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn tls_modes_mark_session_cookies_secure() {
        let mut config = Config::default();
        config.tls.mode = TlsMode::SelfSigned;
        let router =
            Application::builder()
                .auth_pages(
                    get(|context: LoginPageContext| async move {
                        context.csrf_token.as_str().to_owned()
                    }),
                    get(|context: PasswordChangePageContext| async move {
                        context.csrf_token.as_str().to_owned()
                    }),
                )
                .expect("auth pages")
                .into_router(Arc::new(config), auth_database().await);
        let response = router
            .oneshot(Request::get("/login").body(Body::empty()).expect("request"))
            .await
            .expect("login page");
        let session_cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with("webstack.sid="))
            .expect("session cookie");
        assert!(session_cookie.contains("; Secure"), "{session_cookie}");
    }

    #[tokio::test]
    async fn anonymous_htmx_and_csrf_failures_have_protocol_aware_statuses() {
        let router = Application::builder()
            .authenticated_route("/account", get(|| async { "account" }))
            .expect("protected route")
            .route("/unsafe", post(|| async { "changed" }))
            .expect("unsafe route")
            .into_router(Arc::new(Config::default()), test_database().await);
        let anonymous = router
            .clone()
            .oneshot(
                Request::get("/account")
                    .header("hx-request", "true")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("anonymous response");
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(anonymous.headers()["hx-redirect"], "/login");

        let csrf_failure = router
            .oneshot(
                Request::post("/unsafe")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("csrf response");
        assert_eq!(csrf_failure.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn application_errors_preserve_status_select_protocol_and_redact_sources() {
        let router = Application::builder()
            .route(
                "/bad",
                get(|| async { Err::<(), _>(AppError::bad_request("Bad <input>.")) }),
            )
            .expect("bad route")
            .route(
                "/forbidden",
                get(|| async { Err::<(), _>(AppError::forbidden("No access.")) }),
            )
            .expect("forbidden route")
            .route(
                "/missing",
                get(|| async { Err::<(), _>(AppError::not_found("Missing.")) }),
            )
            .expect("missing route")
            .route(
                "/conflict",
                get(|| async { Err::<(), _>(AppError::conflict("Changed.")) }),
            )
            .expect("conflict route")
            .route(
                "/validation",
                get(|| async { Err::<(), _>(AppError::validation("Invalid.")) }),
            )
            .expect("validation route")
            .route(
                "/internal",
                get(|| async {
                    Err::<(), _>(AppError::internal(
                        "test operation",
                        std::io::Error::other("secret database detail"),
                    ))
                }),
            )
            .expect("internal route")
            .into_router(Arc::new(Config::default()), test_database().await);
        let cases = [
            ("/bad", StatusCode::BAD_REQUEST),
            ("/forbidden", StatusCode::FORBIDDEN),
            ("/missing", StatusCode::NOT_FOUND),
            ("/conflict", StatusCode::CONFLICT),
            ("/validation", StatusCode::UNPROCESSABLE_ENTITY),
            ("/internal", StatusCode::INTERNAL_SERVER_ERROR),
        ];
        for (path, status) in cases {
            let response = router
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).expect("request"))
                .await
                .expect("error response");
            assert_eq!(response.status(), status);
            let body = String::from_utf8(
                to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("error body")
                    .to_vec(),
            )
            .expect("UTF-8 error body");
            assert!(body.starts_with("<!doctype html>"), "{body}");
            assert!(!body.contains("secret database detail"), "{body}");
        }

        let partial = router
            .oneshot(
                Request::get("/bad")
                    .header("hx-request", "true")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("partial response");
        let body = String::from_utf8(
            to_bytes(partial.into_body(), usize::MAX)
                .await
                .expect("partial body")
                .to_vec(),
        )
        .expect("UTF-8 partial body");
        assert!(body.starts_with("<div class=\"alert"), "{body}");
        assert!(body.contains("Bad &#60;input&#62;."), "{body}");
        assert!(!body.contains("Bad <input>."), "{body}");
    }

    #[tokio::test]
    async fn custom_error_renderer_can_render_or_select_the_framework_fallback() {
        let custom = Application::builder()
            .error_renderer(|view| {
                Some(format!(
                    "custom:{}:{}",
                    view.status().as_u16(),
                    view.is_htmx()
                ))
            })
            .expect("custom renderer")
            .route(
                "/error",
                get(|| async { Err::<(), _>(AppError::not_found("Missing.")) }),
            )
            .expect("error route")
            .into_router(Arc::new(Config::default()), test_database().await);
        let response = custom
            .oneshot(
                Request::get("/error")
                    .header("hx-request", "true")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("custom response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("custom body"),
            "custom:404:true"
        );

        let fallback = Application::builder()
            .error_renderer(|_view| None)
            .expect("fallback renderer")
            .route(
                "/error",
                get(|| async { Err::<(), _>(AppError::conflict("Try again.")) }),
            )
            .expect("error route")
            .into_router(Arc::new(Config::default()), test_database().await);
        let response = fallback
            .oneshot(Request::get("/error").body(Body::empty()).expect("request"))
            .await
            .expect("fallback response");
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("fallback body");
        assert!(body.starts_with(b"<!doctype html>"));
    }

    #[test]
    fn deferred_backup_fails_clearly() {
        let mut config = Config::default();
        config.backup.enabled = true;
        assert!(matches!(
            validate_runtime_features(&config),
            Err(ApplicationError::UnsupportedFeature("backup"))
        ));
    }
}
