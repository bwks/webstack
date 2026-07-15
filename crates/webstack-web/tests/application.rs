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

#[test]
fn authentication_paths_are_reserved_and_pages_register_once() {
    let reserved = Application::builder().route("/login", get(|| async { "custom login" }));
    assert!(matches!(
        reserved,
        Err(ApplicationError::ReservedRoute(path)) if path == "/login"
    ));

    let duplicate = Application::builder()
        .auth_pages(get(|| async { "login" }), get(|| async { "password" }))
        .expect("first authentication pages")
        .auth_pages(get(|| async { "login" }), get(|| async { "password" }));
    assert!(matches!(
        duplicate,
        Err(ApplicationError::AuthPagesAlreadyRegistered)
    ));
}

#[test]
fn role_routes_validate_public_role_names() {
    let invalid = Application::builder().role_route(
        "/admin",
        "Admin-User",
        get(|| async { "administrator" }),
    );
    assert!(matches!(
        invalid,
        Err(ApplicationError::InvalidRole(role)) if role == "Admin-User"
    ));

    Application::builder()
        .role_route("/reports", "report_editor", get(|| async { "reports" }))
        .expect("lowercase snake-case role");
}

#[test]
fn application_error_renderer_registers_once() {
    let duplicate = Application::builder()
        .error_renderer(|_view| None)
        .expect("first renderer")
        .error_renderer(|_view| None);
    assert!(matches!(
        duplicate,
        Err(ApplicationError::ErrorRendererAlreadyRegistered)
    ));
}
