use webstack::{Application, ApplicationBuilder, axum::routing::get};

use super::{assets::Assets, auth, errors, home, items};

/// Builds the complete demo-owned HTTP route graph.
pub(crate) fn build() -> Result<ApplicationBuilder, webstack::ApplicationError> {
    let application = Application::builder()
        .assets::<Assets>()?
        .error_renderer(errors::render)?
        .auth_pages(get(auth::login), get(auth::change_password))?
        .route("/", get(home::index))?
        .authenticated_route("/account", get(home::account))?
        .role_route("/admin", "admin", get(home::admin))?;
    items::routes(application)
}
