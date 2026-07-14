#![doc = "The supported facade for applications built with Webstack."]

pub use askama;
pub use askama_web;
pub use rust_embed;
pub use serde;
pub use tokio;
pub use tracing;
pub use webstack_core::{config, observability};
pub use webstack_web::{AppState, Application, ApplicationBuilder, ApplicationError, assets, axum};

pub mod prelude {
    pub use webstack_core::config::Config;
    pub use webstack_core::observability::{LogFormat, ObservabilityConfig};
    pub use webstack_web::{AppState, Application, ApplicationError};
}
