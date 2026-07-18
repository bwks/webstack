mod handlers;
mod views;

use webstack::{
    ApplicationBuilder,
    axum::routing::{get, post, put},
};

const ITEM_ROLE: &str = "user";

/// Adds the shared Items reference feature to the demo application.
pub(crate) fn routes(
    application: ApplicationBuilder,
) -> Result<ApplicationBuilder, webstack::ApplicationError> {
    application
        .authenticated_route("/items", get(handlers::index))?
        .role_route("/items", ITEM_ROLE, post(handlers::create))?
        .authenticated_route("/items/{id}/edit", get(handlers::edit))?
        .role_route(
            "/items/{id}",
            ITEM_ROLE,
            put(handlers::update)
                .delete(handlers::remove)
                .post(handlers::update),
        )?
        .role_route("/items/{id}/delete", ITEM_ROLE, post(handlers::remove))
}
