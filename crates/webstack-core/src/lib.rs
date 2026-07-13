#![doc = "Application lifecycle and composition primitives."]

pub mod observability;

#[derive(Debug, Default)]
pub struct Application;

impl Application {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}
