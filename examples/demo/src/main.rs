use webstack::axum::routing::get;
use webstack::prelude::*;

/// Renders the demo application's root response.
async fn index() -> &'static str {
    "Webstack demo"
}

#[webstack::tokio::main(crate = "webstack::tokio")]
/// Composes and runs the demonstration application.
async fn main() -> anyhow::Result<()> {
    Application::builder().route("/", get(index))?.run().await?;
    Ok(())
}
