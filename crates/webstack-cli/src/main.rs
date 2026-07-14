use std::process::ExitCode;

use anyhow::Context;
use webstack_core::observability::{self, LogFormat, ObservabilityConfig, ObservabilityError};

#[tokio::main]
async fn main() -> ExitCode {
    match run_command().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run_command() -> anyhow::Result<()> {
    let cli = webstack_cli::Cli::parse_args();
    init_tracing(cli.verbosity()).context("could not initialize CLI tracing")?;
    cli.execute().await.context("webstack command failed")
}

fn init_tracing(verbosity: u8) -> Result<(), ObservabilityError> {
    observability::init(&ObservabilityConfig::new(
        verbosity_filter(verbosity),
        LogFormat::Pretty,
    ))
}

const fn verbosity_filter(verbosity: u8) -> &'static str {
    match verbosity {
        0 => "warn",
        1 => "debug",
        _ => "trace",
    }
}

#[cfg(test)]
mod tests {
    use super::verbosity_filter;

    #[test]
    fn verbosity_maps_to_increasing_diagnostic_levels() {
        assert_eq!(verbosity_filter(0), "warn");
        assert_eq!(verbosity_filter(1), "debug");
        assert_eq!(verbosity_filter(2), "trace");
        assert_eq!(verbosity_filter(u8::MAX), "trace");
    }
}
