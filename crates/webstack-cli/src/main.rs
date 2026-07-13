use std::process::ExitCode;

use anyhow::Context;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt;

fn main() -> ExitCode {
    match run_command() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run_command() -> anyhow::Result<()> {
    let cli = webstack_cli::Cli::parse_args();
    init_tracing(cli.verbosity())
        .map_err(anyhow::Error::from_boxed)
        .context("could not initialize CLI tracing")?;
    cli.execute().context("webstack command failed")
}

fn init_tracing(verbosity: u8) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    fmt()
        .with_max_level(verbosity_filter(verbosity))
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_writer(std::io::stderr)
        .compact()
        .try_init()
}

const fn verbosity_filter(verbosity: u8) -> LevelFilter {
    match verbosity {
        0 => LevelFilter::WARN,
        1 => LevelFilter::DEBUG,
        _ => LevelFilter::TRACE,
    }
}

#[cfg(test)]
mod tests {
    use tracing::level_filters::LevelFilter;

    use super::verbosity_filter;

    #[test]
    fn verbosity_maps_to_increasing_diagnostic_levels() {
        assert_eq!(verbosity_filter(0), LevelFilter::WARN);
        assert_eq!(verbosity_filter(1), LevelFilter::DEBUG);
        assert_eq!(verbosity_filter(2), LevelFilter::TRACE);
        assert_eq!(verbosity_filter(u8::MAX), LevelFilter::TRACE);
    }
}
