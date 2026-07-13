use std::{future::Future, net::SocketAddr, sync::Arc};

use axum::{
    Json, Router,
    routing::{MethodRouter, get},
};
use serde::Serialize;
use thiserror::Error;
use tokio::net::TcpListener;
use webstack_core::{
    config::{Config, ConfigError, TlsMode},
    observability::{self, ObservabilityError},
};

const HEALTH_PATH: &str = "/healthz";

/// Entry point for composing one generated application's HTTP runtime.
pub struct Application;

impl Application {
    #[must_use]
    pub fn builder() -> ApplicationBuilder {
        ApplicationBuilder {
            router: Router::new(),
            route_count: 0,
        }
    }
}

/// Fluent application-owned route builder.
pub struct ApplicationBuilder {
    router: Router<AppState>,
    route_count: usize,
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
        if path == HEALTH_PATH {
            return Err(ApplicationError::ReservedRoute(path.to_owned()));
        }
        self.router = self.router.route(path, method_router);
        self.route_count += 1;
        Ok(self)
    }

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

        let address = SocketAddr::new(config.server.bind_addr, config.server.http_port);
        let listener = TcpListener::bind(address)
            .await
            .map_err(|source| ApplicationError::Bind { address, source })?;
        let local_address = listener
            .local_addr()
            .map_err(|source| ApplicationError::Bind { address, source })?;
        tracing::info!(%local_address, "HTTP server listening");

        let router = self.into_router(Arc::new(config));
        serve(listener, router, shutdown_signal()).await
    }

    fn into_router(self, config: Arc<Config>) -> Router {
        self.router
            .route(HEALTH_PATH, get(health))
            .with_state(AppState { config })
    }
}

/// Framework state available to application handlers through Axum `State`.
pub struct AppState {
    config: Arc<Config>,
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
        }
    }
}

impl AppState {
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}

fn validate_runtime_features(config: &Config) -> Result<(), ApplicationError> {
    if config.tls.mode != TlsMode::Disabled {
        return Err(ApplicationError::UnsupportedFeature("TLS"));
    }
    if config.backup.enabled {
        return Err(ApplicationError::UnsupportedFeature("backup"));
    }
    Ok(())
}

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

/// A typed application composition or runtime failure.
#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("route {0:?} is reserved by Webstack")]
    ReservedRoute(String),
    #[error("{0} support is not implemented yet; disable it in configuration")]
    UnsupportedFeature(&'static str),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Observability(#[from] ObservabilityError),
    #[error("cannot bind HTTP listener at {address}")]
    Bind {
        address: SocketAddr,
        #[source]
        source: std::io::Error,
    },
    #[error("HTTP server failed")]
    Serve(#[source] std::io::Error),
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
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::oneshot,
    };
    use tower::ServiceExt;
    use webstack_core::config::{Config, TlsMode};

    use super::{AppState, Application, ApplicationError, serve, validate_runtime_features};

    #[tokio::test]
    async fn health_is_minimal_json_and_webstack_state_is_available() {
        let router = Application::builder()
            .route(
                "/",
                get(|State(state): State<AppState>| async move {
                    state.config().server.http_port.to_string()
                }),
            )
            .expect("application route")
            .into_router(Arc::new(Config::default()));

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
        let router = Application::builder().into_router(Arc::new(Config::default()));
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
