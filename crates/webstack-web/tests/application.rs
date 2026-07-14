use webstack_web::{Application, ApplicationError, axum::routing::get};

#[derive(rust_embed::RustEmbed)]
#[folder = "tests/fixtures/assets"]
struct Assets;

#[test]
fn health_route_is_reserved() {
    let result = Application::builder().route("/healthz", get(|| async { "application health" }));

    assert!(matches!(result, Err(ApplicationError::ReservedRoute(path)) if path == "/healthz"));
}

#[test]
fn static_routes_are_reserved_and_assets_register_once() {
    let reserved =
        Application::builder().route("/static/custom.css", get(|| async { "application asset" }));
    assert!(matches!(
        reserved,
        Err(ApplicationError::ReservedRoute(path)) if path == "/static/custom.css"
    ));

    let duplicate = Application::builder()
        .assets::<Assets>()
        .expect("first asset collection")
        .assets::<Assets>();
    assert!(matches!(
        duplicate,
        Err(ApplicationError::AssetsAlreadyRegistered)
    ));
}

#[test]
fn application_routes_compose_fluently() {
    let builder = Application::builder()
        .route("/", get(|| async { "application root" }))
        .expect("application route");

    assert_eq!(builder.route_count(), 1);
}
