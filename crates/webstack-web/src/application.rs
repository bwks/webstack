use std::{convert::Infallible, future::Future, net::SocketAddr, path::Path, sync::Arc};

use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::{MethodRouter, get},
};
use axum_login::AuthManagerLayerBuilder;
use serde::Serialize;
use thiserror::Error;
use tokio::net::TcpListener;
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
        }
    }
}

/// Fluent application-owned route builder.
pub struct ApplicationBuilder {
    router: Router<AppState>,
    route_count: usize,
    assets_registered: bool,
    auth_pages_registered: bool,
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
    /// Returns a typed startup or server failure. TLS and backups fail clearly
    /// until their implementation milestones are complete.
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

        let address = SocketAddr::new(config.server.bind_addr, config.server.http_port);
        let listener = TcpListener::bind(address)
            .await
            .map_err(|source| ApplicationError::Bind { address, source })?;
        let local_address = listener
            .local_addr()
            .map_err(|source| ApplicationError::Bind { address, source })?;
        tracing::info!(%local_address, "HTTP server listening");

        let cleanup = spawn_session_cleanup(database.clone());
        let router = self.into_router(Arc::new(config), database);
        let result = serve(listener, router, shutdown_signal()).await;
        cleanup.abort();
        result
    }

    /// Converts the builder into a stateful Axum router with framework routes.
    fn into_router(self, config: Arc<Config>, database: Database) -> Router {
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
    #[error("cannot bind HTTP listener at {address}")]
    Bind {
        address: SocketAddr,
        #[source]
        source: std::io::Error,
    },
    #[error("HTTP server failed")]
    Serve(#[source] std::io::Error),
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
    if config.tls.mode != TlsMode::Disabled {
        return Err(ApplicationError::UnsupportedFeature("TLS"));
    }
    if config.backup.enabled {
        return Err(ApplicationError::UnsupportedFeature("backup"));
    }
    Ok(())
}

/// Serves a router until the supplied shutdown future completes.
async fn serve(
    listener: TcpListener,
    router: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ApplicationError> {
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
    .map_err(ApplicationError::Serve)
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
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::oneshot,
    };
    use tower::ServiceExt;
    use webstack_auth::{LoginPageContext, PasswordChangePageContext, bootstrap_admin};
    use webstack_core::config::{Config, Environment, TlsMode};
    use webstack_db::Database;

    use super::{AppState, Application, ApplicationError, serve, validate_runtime_features};

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
                    .header(header::COOKIE, cookies)
                    .body(Body::from(form))
                    .expect("login request"),
            )
            .await
            .expect("login response");
        assert_eq!(login.status(), StatusCode::SEE_OTHER);
        assert_eq!(login.headers()[header::LOCATION], "/");
        let authenticated_cookies = response_cookies(&login);

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

    #[test]
    fn deferred_features_fail_clearly() {
        let mut config = Config::default();
        config.tls.mode = TlsMode::SelfSigned;
        assert!(matches!(
            validate_runtime_features(&config),
            Err(ApplicationError::UnsupportedFeature("TLS"))
        ));

        config.tls.mode = TlsMode::Disabled;
        config.backup.enabled = true;
        assert!(matches!(
            validate_runtime_features(&config),
            Err(ApplicationError::UnsupportedFeature("backup"))
        ));
    }

    #[tokio::test]
    async fn real_listener_serves_and_stops_on_injected_shutdown() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("address");
        let router =
            Application::builder().into_router(Arc::new(Config::default()), test_database().await);
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(serve(listener, router, async move {
            let _result = shutdown_receiver.await;
        }));

        let mut connection = TcpStream::connect(address).await.expect("connect");
        connection
            .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .expect("request");
        let mut response = Vec::new();
        connection
            .read_to_end(&mut response)
            .await
            .expect("response");
        let response = String::from_utf8(response).expect("UTF-8 response");
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.ends_with("{\"status\":\"ok\"}"), "{response}");

        shutdown_sender.send(()).expect("shutdown");
        server.await.expect("server task").expect("server result");
    }
}
