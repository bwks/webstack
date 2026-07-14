use std::{future::Future, net::SocketAddr, path::Path, sync::Arc};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{MethodRouter, get},
};
use serde::Serialize;
use thiserror::Error;
use tokio::net::TcpListener;
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
        }
    }
}

/// Fluent application-owned route builder.
pub struct ApplicationBuilder {
    router: Router<AppState>,
    route_count: usize,
    assets_registered: bool,
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
        if path == HEALTH_PATH || path == STATIC_PATH || path.starts_with("/static/") {
            return Err(ApplicationError::ReservedRoute(path.to_owned()));
        }
        self.router = self.router.route(path, method_router);
        self.route_count += 1;
        Ok(self)
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

        let address = SocketAddr::new(config.server.bind_addr, config.server.http_port);
        let listener = TcpListener::bind(address)
            .await
            .map_err(|source| ApplicationError::Bind { address, source })?;
        let local_address = listener
            .local_addr()
            .map_err(|source| ApplicationError::Bind { address, source })?;
        tracing::info!(%local_address, "HTTP server listening");

        let router = self.into_router(Arc::new(config), database);
        serve(listener, router, shutdown_signal()).await
    }

    /// Converts the builder into a stateful Axum router with framework routes.
    fn into_router(self, config: Arc<Config>, database: Database) -> Router {
        self.router
            .route(HEALTH_PATH, get(health))
            .with_state(AppState { config, database })
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
    #[error("{0} support is not implemented yet; disable it in configuration")]
    UnsupportedFeature(&'static str),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Observability(#[from] ObservabilityError),
    #[error(transparent)]
    Database(#[from] DatabaseError),
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
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(ApplicationError::Serve)
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
        http::{Request, StatusCode},
        routing::get,
    };
    use surrealdb::{Surreal, engine::local::Mem};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::oneshot,
    };
    use tower::ServiceExt;
    use webstack_core::config::{Config, TlsMode};
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
