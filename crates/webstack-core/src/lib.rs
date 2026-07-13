#![doc = "Application lifecycle and composition primitives."]

#[derive(Debug, Default)]
pub struct Application;

impl Application {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}
