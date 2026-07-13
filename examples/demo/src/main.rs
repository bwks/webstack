use webstack::prelude::*;

fn main() -> anyhow::Result<()> {
    webstack::observability::init(&ObservabilityConfig::default())?;
    let _app = Application::new();
    webstack::tracing::info!(application = "webstack-demo", "application composed");
    Ok(())
}
