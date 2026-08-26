#![doc = "The supported facade for applications built with Webstack."]

pub use askama;
pub use askama_web;
pub use rust_embed;
pub use serde;
pub use tokio;
pub use tracing;
pub use turso;
pub use webstack_core::{config, observability};
pub use webstack_web::{
    AppError, AppState, Application, ApplicationBuilder, ApplicationError, ErrorView, assets, axum,
};

pub mod htmx {
    //! Supported htmx request metadata for application handlers.

    pub use webstack_web::htmx::HxRequest;
}

pub mod tls {
    //! Supported TLS runtime errors.

    pub use webstack_web::tls::TlsError;
}

pub mod auth {
    //! Supported authentication API for application handlers and templates.

    pub use webstack_auth::{
        ADMIN_ROLE, AuthMessage, AuthSession, CsrfToken, LoginPageContext, NewUser,
        PasswordChangePageContext, USER_ROLE, User,
    };
}

pub mod database {
    //! Supported embedded database API for application code.

    pub use webstack_db::{Database, DatabaseError, retry_write};
}

pub mod prelude {
    pub use webstack_auth::{CsrfToken, LoginPageContext, PasswordChangePageContext};
    pub use webstack_core::config::Config;
    pub use webstack_core::observability::{LogFormat, ObservabilityConfig};
    pub use webstack_db::Database;
    pub use webstack_web::{
        AppError, AppState, Application, ApplicationError, ErrorView, htmx::HxRequest,
    };
}
