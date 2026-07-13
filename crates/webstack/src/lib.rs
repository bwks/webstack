#![doc = "The supported facade for applications built with Webstack."]

pub use tracing;
pub use webstack_core::Application;
pub use webstack_core::observability;

pub mod prelude {
    pub use webstack_core::Application;
    pub use webstack_core::observability::{LogFormat, ObservabilityConfig};
}
