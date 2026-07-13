use webstack_web::{Application, ApplicationError, axum::routing::get};

#[test]
fn health_route_is_reserved() {
    let result = Application::builder().route("/healthz", get(|| async { "application health" }));

    assert!(matches!(result, Err(ApplicationError::ReservedRoute(path)) if path == "/healthz"));
}

#[test]
fn application_routes_compose_fluently() {
    let builder = Application::builder()
        .route("/", get(|| async { "application root" }))
        .expect("application route");

    assert_eq!(builder.route_count(), 1);
}
