use crate::web;

/// Composes and runs the demonstration application.
pub(crate) async fn run() -> anyhow::Result<()> {
    web::router::build()?.run().await?;
    Ok(())
}
