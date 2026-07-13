use webstack::axum::routing::get;
use webstack::prelude::*;

async fn index() -> &'static str {
    "Webstack demo"
}

#[webstack::tokio::main(crate = "webstack::tokio")]
async fn main() -> anyhow::Result<()> {
    Application::builder().route("/", get(index))?.run().await?;
    Ok(())
}
