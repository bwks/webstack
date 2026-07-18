mod application;
mod domain;
mod web;

#[webstack::tokio::main(crate = "webstack::tokio")]
/// Starts the demonstration application.
async fn main() -> anyhow::Result<()> {
    application::run().await
}
