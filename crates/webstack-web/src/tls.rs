use std::{
    future::{Future, pending},
    io,
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{StatusCode, header, uri::Authority},
    response::{IntoResponse, Response},
};
use axum_server::{
    Handle,
    tls_rustls::{RustlsConfig, from_tcp_rustls},
};
use futures_util::StreamExt;
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls_acme::{AcmeConfig, AcmeState, axum::AxumAcceptor, caches::DirCache};
use thiserror::Error;
use tokio::{
    net::TcpListener,
    task::{JoinError, JoinHandle, JoinSet},
};
use webstack_core::config::{Config, TlsMode};

const HTTP_SCHEME: &str = "http";
const HTTPS_SCHEME: &str = "https";
const PRIMARY_LISTENER: &str = "application";
const REDIRECT_LISTENER: &str = "redirect";

/// A typed TLS listener, certificate, or ACME runtime failure.
#[derive(Debug, Error)]
pub enum TlsError {
    #[error("cannot bind {listener} listener at {address}")]
    Bind {
        listener: &'static str,
        address: SocketAddr,
        #[source]
        source: io::Error,
    },
    #[error("cannot configure the {listener} listener at {address}")]
    ConfigureListener {
        listener: &'static str,
        address: SocketAddr,
        #[source]
        source: io::Error,
    },
    #[error("cannot generate the ephemeral self-signed certificate")]
    SelfSignedCertificate(#[source] rcgen::Error),
    #[error("cannot configure rustls with the self-signed certificate")]
    SelfSignedRustls(#[source] io::Error),
    #[error("{listener} listener at {address} failed")]
    Serve {
        listener: &'static str,
        address: SocketAddr,
        #[source]
        source: io::Error,
    },
    #[error("{listener} listener at {address} stopped unexpectedly")]
    ListenerStopped {
        listener: &'static str,
        address: SocketAddr,
    },
    #[error("the listener task set stopped unexpectedly")]
    ListenerSetStopped,
    #[error("the ACME certificate event stream stopped unexpectedly")]
    AcmeStopped,
    #[error("{task} task failed")]
    Task {
        task: &'static str,
        #[source]
        source: JoinError,
    },
}

struct BoundListener {
    listener: std::net::TcpListener,
    address: SocketAddr,
}

enum PrimaryMode {
    Http,
    SelfSigned(RustlsConfig),
    Acme(AcmeRuntime),
}

struct AcmeRuntime {
    acceptor: AxumAcceptor,
    state: AcmeState<io::Error>,
}

#[derive(Clone)]
struct RedirectState {
    host: RedirectHost,
    https_port: u16,
}

#[derive(Clone)]
enum RedirectHost {
    Configured(String),
    Request,
}

#[derive(Clone, Copy)]
struct ListenerIdentity {
    name: &'static str,
    address: SocketAddr,
}

/// Serves one application using the configured HTTP and TLS listener topology.
pub(crate) async fn serve(
    config: &Config,
    router: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), TlsError> {
    let mode = primary_mode(config).await?;
    let primary_port = match config.tls.mode {
        TlsMode::Disabled => config.server.http_port,
        TlsMode::Acme | TlsMode::SelfSigned => config.server.https_port,
    };
    let primary = bind_listener(
        PRIMARY_LISTENER,
        SocketAddr::new(config.server.bind_addr, primary_port),
    )
    .await?;
    let redirect = if config.tls.mode != TlsMode::Disabled && config.server.http_redirect {
        Some(
            bind_listener(
                REDIRECT_LISTENER,
                SocketAddr::new(config.server.bind_addr, config.server.http_port),
            )
            .await?,
        )
    } else {
        None
    };

    let primary_handle = Handle::new();
    let redirect_handle = redirect.as_ref().map(|_| Handle::new());
    let mut acme_task = None;
    let mut servers = JoinSet::new();
    start_primary(
        &mut servers,
        primary,
        mode,
        primary_handle.clone(),
        router,
        &mut acme_task,
    )?;
    if let (Some(redirect), Some(handle)) = (redirect, redirect_handle.as_ref()) {
        start_redirect(
            &mut servers,
            redirect,
            handle.clone(),
            redirect_state(config),
        )?;
    }

    let runtime_error = tokio::select! {
        () = shutdown => None,
        exit = servers.join_next() => Some(server_exit(exit)),
        result = wait_for_acme(&mut acme_task) => Some(result),
    };
    let shutdown_timeout = Some(Duration::from_secs(config.server.shutdown_timeout_seconds));
    primary_handle.graceful_shutdown(shutdown_timeout);
    if let Some(handle) = redirect_handle {
        handle.graceful_shutdown(shutdown_timeout);
    }
    let mut shutdown_error = None;
    while let Some(exit) = servers.join_next().await {
        if runtime_error.is_none() && shutdown_error.is_none() {
            shutdown_error = server_shutdown_result(exit).err();
        }
    }
    if let Some(task) = acme_task {
        task.abort();
    }
    if let Some(result) = runtime_error {
        result
    } else if let Some(error) = shutdown_error {
        Err(error)
    } else {
        Ok(())
    }
}

/// Creates the configured primary HTTP, self-signed, or ACME serving mode.
async fn primary_mode(config: &Config) -> Result<PrimaryMode, TlsError> {
    match config.tls.mode {
        TlsMode::Disabled => Ok(PrimaryMode::Http),
        TlsMode::SelfSigned => self_signed_mode().await.map(PrimaryMode::SelfSigned),
        TlsMode::Acme => Ok(PrimaryMode::Acme(acme_mode(config))),
    }
}

/// Generates an ephemeral localhost certificate and matching rustls configuration.
async fn self_signed_mode() -> Result<RustlsConfig, TlsError> {
    let CertifiedKey { cert, signing_key } = generate_simple_self_signed(["localhost".to_owned()])
        .map_err(TlsError::SelfSignedCertificate)?;
    RustlsConfig::from_der(
        vec![cert.der().as_ref().to_vec()],
        signing_key.serialize_der(),
    )
    .await
    .map_err(TlsError::SelfSignedRustls)
}

/// Creates an ACME acceptor and its continuously polled certificate state.
fn acme_mode(config: &Config) -> AcmeRuntime {
    let domain = config.tls.domain.as_deref().expect("validated ACME domain");
    let email = config
        .tls
        .acme_email
        .as_deref()
        .expect("validated ACME email");
    let state = AcmeConfig::new([domain])
        .contact_push(format!("mailto:{email}"))
        .cache(DirCache::new(config.tls.acme_cache_dir.clone()))
        .directory_lets_encrypt(!config.tls.acme_staging)
        .state();
    let mut rustls_config = state.default_rustls_config();
    Arc::get_mut(&mut rustls_config)
        .expect("new ACME rustls configuration is uniquely owned")
        .alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let acceptor = state.axum_acceptor(rustls_config);
    AcmeRuntime { acceptor, state }
}

/// Binds one listener without beginning to accept requests.
async fn bind_listener(
    listener_name: &'static str,
    address: SocketAddr,
) -> Result<BoundListener, TlsError> {
    let listener = TcpListener::bind(address)
        .await
        .map_err(|source| TlsError::Bind {
            listener: listener_name,
            address,
            source,
        })?;
    let address = listener.local_addr().map_err(|source| TlsError::Bind {
        listener: listener_name,
        address,
        source,
    })?;
    let listener = listener
        .into_std()
        .map_err(|source| TlsError::ConfigureListener {
            listener: listener_name,
            address,
            source,
        })?;
    Ok(BoundListener { listener, address })
}

/// Starts the application listener and optional ACME event task.
fn start_primary(
    servers: &mut JoinSet<Result<ListenerIdentity, TlsError>>,
    listener: BoundListener,
    mode: PrimaryMode,
    handle: Handle<SocketAddr>,
    router: Router,
    acme_task: &mut Option<JoinHandle<Result<(), TlsError>>>,
) -> Result<(), TlsError> {
    let identity = ListenerIdentity {
        name: PRIMARY_LISTENER,
        address: listener.address,
    };
    let service = router.into_make_service_with_connect_info::<SocketAddr>();
    match mode {
        PrimaryMode::Http => {
            tracing::info!(address = %identity.address, scheme = HTTP_SCHEME, "application server listening");
            let server = axum_server::from_tcp(listener.listener)
                .map_err(|source| configure_error(identity, source))?
                .handle(handle);
            servers.spawn(async move {
                server
                    .serve(service)
                    .await
                    .map_err(|source| serve_error(identity, source))?;
                Ok(identity)
            });
        }
        PrimaryMode::SelfSigned(config) => {
            tracing::info!(address = %identity.address, scheme = HTTPS_SCHEME, certificate = "self-signed", "application server listening");
            let server = from_tcp_rustls(listener.listener, config)
                .map_err(|source| configure_error(identity, source))?
                .handle(handle);
            servers.spawn(async move {
                server
                    .serve(service)
                    .await
                    .map_err(|source| serve_error(identity, source))?;
                Ok(identity)
            });
        }
        PrimaryMode::Acme(runtime) => {
            tracing::info!(address = %identity.address, scheme = HTTPS_SCHEME, certificate = "acme", "application server listening");
            let server = axum_server::from_tcp(listener.listener)
                .map_err(|source| configure_error(identity, source))?
                .acceptor(runtime.acceptor)
                .handle(handle);
            servers.spawn(async move {
                server
                    .serve(service)
                    .await
                    .map_err(|source| serve_error(identity, source))?;
                Ok(identity)
            });
            *acme_task = Some(tokio::spawn(poll_acme(runtime.state)));
        }
    }
    Ok(())
}

/// Starts the optional plain-HTTP redirect listener.
fn start_redirect(
    servers: &mut JoinSet<Result<ListenerIdentity, TlsError>>,
    listener: BoundListener,
    handle: Handle<SocketAddr>,
    state: RedirectState,
) -> Result<(), TlsError> {
    let identity = ListenerIdentity {
        name: REDIRECT_LISTENER,
        address: listener.address,
    };
    tracing::info!(address = %identity.address, scheme = HTTP_SCHEME, "HTTPS redirect server listening");
    let router = Router::new().fallback(redirect_to_https).with_state(state);
    let server = axum_server::from_tcp(listener.listener)
        .map_err(|source| configure_error(identity, source))?
        .handle(handle);
    servers.spawn(async move {
        server
            .serve(router.into_make_service())
            .await
            .map_err(|source| serve_error(identity, source))?;
        Ok(identity)
    });
    Ok(())
}

/// Polls ACME certificate events for acquisition and renewal.
async fn poll_acme(mut state: AcmeState<io::Error>) -> Result<(), TlsError> {
    while let Some(event) = state.next().await {
        match event {
            Ok(event) => tracing::info!(?event, "ACME certificate event"),
            Err(error) => tracing::error!(%error, "ACME certificate event failed"),
        }
    }
    Err(TlsError::AcmeStopped)
}

/// Waits for an ACME task or remains pending when ACME is not active.
async fn wait_for_acme(
    task: &mut Option<JoinHandle<Result<(), TlsError>>>,
) -> Result<(), TlsError> {
    match task {
        Some(task) => task.await.map_err(|source| TlsError::Task {
            task: "ACME",
            source,
        })?,
        None => pending().await,
    }
}

/// Converts an early listener exit into a runtime failure.
fn server_exit(
    exit: Option<Result<Result<ListenerIdentity, TlsError>, JoinError>>,
) -> Result<(), TlsError> {
    match exit {
        Some(Ok(Ok(identity))) => Err(TlsError::ListenerStopped {
            listener: identity.name,
            address: identity.address,
        }),
        Some(Ok(Err(error))) => Err(error),
        Some(Err(source)) => Err(TlsError::Task {
            task: "listener",
            source,
        }),
        None => Err(TlsError::ListenerSetStopped),
    }
}

/// Validates one listener result after graceful shutdown was requested.
fn server_shutdown_result(
    exit: Result<Result<ListenerIdentity, TlsError>, JoinError>,
) -> Result<(), TlsError> {
    match exit {
        Ok(Ok(_identity)) => Ok(()),
        Ok(Err(error)) => Err(error),
        Err(source) => Err(TlsError::Task {
            task: "listener",
            source,
        }),
    }
}

/// Creates the typed listener configuration error for one server identity.
fn configure_error(identity: ListenerIdentity, source: io::Error) -> TlsError {
    TlsError::ConfigureListener {
        listener: identity.name,
        address: identity.address,
        source,
    }
}

/// Creates the typed serving error for one listener identity.
fn serve_error(identity: ListenerIdentity, source: io::Error) -> TlsError {
    TlsError::Serve {
        listener: identity.name,
        address: identity.address,
        source,
    }
}

/// Selects the redirect hostname policy for the configured TLS mode.
fn redirect_state(config: &Config) -> RedirectState {
    let host = match config.tls.mode {
        TlsMode::Acme => {
            RedirectHost::Configured(config.tls.domain.clone().expect("validated ACME domain"))
        }
        TlsMode::SelfSigned | TlsMode::Disabled => RedirectHost::Request,
    };
    RedirectState {
        host,
        https_port: config.server.https_port,
    }
}

/// Redirects one plain-HTTP request to the configured HTTPS listener.
async fn redirect_to_https(State(state): State<RedirectState>, request: Request) -> Response {
    let host = match &state.host {
        RedirectHost::Configured(host) => host.clone(),
        RedirectHost::Request => match request_host(&request) {
            Some(host) => host,
            None => return StatusCode::BAD_REQUEST.into_response(),
        },
    };
    let host = bracket_ipv6(&host);
    let authority = if state.https_port == 443 {
        host
    } else {
        format!("{host}:{}", state.https_port)
    };
    let path = request
        .uri()
        .path_and_query()
        .map_or("/", |path| path.as_str());
    let location = format!("{HTTPS_SCHEME}://{authority}{path}");
    let Ok(location) = header::HeaderValue::from_str(&location) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::MOVED_PERMANENTLY;
    response.headers_mut().insert(header::LOCATION, location);
    response
}

/// Extracts a validated hostname without its HTTP listener port.
fn request_host(request: &Request) -> Option<String> {
    request
        .headers()
        .get(header::HOST)?
        .to_str()
        .ok()?
        .parse::<Authority>()
        .ok()
        .map(|authority| authority.host().to_owned())
}

/// Adds URI brackets around an IPv6 host when required.
fn bracket_ipv6(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::TcpListener as StdTcpListener,
        sync::Arc,
        time::{Duration, Instant},
    };

    use axum::{Router, body::Body, http::Request, routing::get};
    use reqwest::redirect::Policy;
    use tempfile::tempdir;
    use tokio::{
        sync::{Notify, oneshot},
        time::sleep,
    };
    use tower::ServiceExt;
    use webstack_core::config::{Config, TlsMode};

    use super::{RedirectHost, RedirectState, acme_mode, redirect_to_https, serve};

    fn unused_port() -> u16 {
        StdTcpListener::bind("127.0.0.1:0")
            .expect("ephemeral listener")
            .local_addr()
            .expect("ephemeral address")
            .port()
    }

    async fn wait_for_response(client: &reqwest::Client, url: &str) -> reqwest::Response {
        let mut last_error = None;
        for _attempt in 0..50 {
            match client.get(url).send().await {
                Ok(response) => return response,
                Err(error) => {
                    last_error = Some(error);
                    sleep(Duration::from_millis(20)).await;
                }
            }
        }
        panic!("listener did not become ready: {last_error:?}");
    }

    #[tokio::test]
    async fn redirects_preserve_targets_and_validate_request_hosts() {
        let configured = Router::new()
            .fallback(redirect_to_https)
            .with_state(RedirectState {
                host: RedirectHost::Configured("app.example.com".to_owned()),
                https_port: 443,
            });
        let response = configured
            .oneshot(
                Request::get("/items?page=2")
                    .body(Body::empty())
                    .expect("configured redirect request"),
            )
            .await
            .expect("configured redirect response");
        assert_eq!(response.status(), 301);
        assert_eq!(
            response.headers()["location"],
            "https://app.example.com/items?page=2"
        );

        let requested = Router::new()
            .fallback(redirect_to_https)
            .with_state(RedirectState {
                host: RedirectHost::Request,
                https_port: 7337,
            });
        let response = requested
            .clone()
            .oneshot(
                Request::get("/account")
                    .header("host", "10.100.58.10:42069")
                    .body(Body::empty())
                    .expect("request-host redirect request"),
            )
            .await
            .expect("request-host redirect response");
        assert_eq!(response.status(), 301);
        assert_eq!(
            response.headers()["location"],
            "https://10.100.58.10:7337/account"
        );

        let response = requested
            .oneshot(
                Request::get("/")
                    .body(Body::empty())
                    .expect("missing-host redirect request"),
            )
            .await
            .expect("missing-host redirect response");
        assert_eq!(response.status(), 400);
    }

    #[tokio::test]
    async fn disabled_mode_serves_http_on_the_http_port() {
        let port = unused_port();
        let mut config = Config::default();
        config.server.bind_addr = "127.0.0.1".parse().expect("loopback address");
        config.server.http_port = port;
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(async move {
            serve(
                &config,
                Router::new().route("/healthz", get(|| async { "ok" })),
                async move {
                    let _result = shutdown_receiver.await;
                },
            )
            .await
        });
        let client = reqwest::Client::new();
        let response =
            wait_for_response(&client, &format!("http://127.0.0.1:{port}/healthz")).await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.expect("HTTP body"), "ok");
        shutdown_sender.send(()).expect("shutdown signal");
        server.await.expect("server task").expect("HTTP server");
    }

    #[tokio::test]
    async fn self_signed_https_and_http_redirect_stop_together() {
        let redirect_port = unused_port();
        let secure_port = unused_port();
        let mut config = Config::default();
        config.server.bind_addr = "127.0.0.1".parse().expect("loopback address");
        config.server.http_port = redirect_port;
        config.server.https_port = secure_port;
        config.server.http_redirect = true;
        config.tls.mode = TlsMode::SelfSigned;
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(async move {
            serve(
                &config,
                Router::new().route("/healthz", get(|| async { "secure" })),
                async move {
                    let _result = shutdown_receiver.await;
                },
            )
            .await
        });
        let secure_client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .expect("HTTPS client");
        let response = wait_for_response(
            &secure_client,
            &format!("https://127.0.0.1:{secure_port}/healthz"),
        )
        .await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.expect("HTTPS body"), "secure");

        let redirect_client = reqwest::Client::builder()
            .redirect(Policy::none())
            .build()
            .expect("HTTP client");
        let response = wait_for_response(
            &redirect_client,
            &format!("http://127.0.0.1:{redirect_port}/items?status=open"),
        )
        .await;
        assert_eq!(response.status(), 301);
        assert_eq!(
            response.headers()["location"],
            format!("https://127.0.0.1:{secure_port}/items?status=open")
        );

        shutdown_sender.send(()).expect("shutdown signal");
        server.await.expect("server task").expect("TLS servers");
    }

    #[tokio::test]
    async fn shutdown_deadline_terminates_a_long_running_request() {
        let port = unused_port();
        let mut config = Config::default();
        config.server.bind_addr = "127.0.0.1".parse().expect("loopback address");
        config.server.http_port = port;
        config.server.shutdown_timeout_seconds = 1;
        let started = Arc::new(Notify::new());
        let handler_started = Arc::clone(&started);
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(async move {
            serve(
                &config,
                Router::new()
                    .route("/ready", get(|| async { "ready" }))
                    .route(
                        "/slow",
                        get(move || {
                            let handler_started = Arc::clone(&handler_started);
                            async move {
                                handler_started.notify_one();
                                sleep(Duration::from_secs(30)).await;
                                "late"
                            }
                        }),
                    ),
                async move {
                    let _result = shutdown_receiver.await;
                },
            )
            .await
        });
        let client = reqwest::Client::new();
        let response = wait_for_response(&client, &format!("http://127.0.0.1:{port}/ready")).await;
        assert_eq!(response.status(), 200);
        let request = tokio::spawn(async move {
            client
                .get(format!("http://127.0.0.1:{port}/slow"))
                .send()
                .await
        });
        started.notified().await;
        let shutdown_started = Instant::now();
        shutdown_sender.send(()).expect("shutdown signal");
        server.await.expect("server task").expect("HTTP server");
        assert!(shutdown_started.elapsed() < Duration::from_secs(3));
        assert!(request.await.expect("request task").is_err());
    }

    #[test]
    fn acme_configuration_builds_without_network_or_cache_writes() {
        let cache = tempdir().expect("cache directory");
        let mut config = Config::default();
        config.tls.mode = TlsMode::Acme;
        config.tls.domain = Some("example.com".to_owned());
        config.tls.acme_email = Some("admin@example.com".to_owned());
        config.tls.acme_cache_dir = cache.path().join("acme");
        config.tls.acme_staging = true;
        let runtime = acme_mode(&config);
        drop(runtime);
        assert!(!config.tls.acme_cache_dir.exists());
    }
}
