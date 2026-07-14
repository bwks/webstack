//! Process-level tracing configuration for Webstack applications.

use std::io::{self, IsTerminal};

use garde::Validate;
use serde::Deserialize;
use thiserror::Error;
use tracing_subscriber::{EnvFilter, filter::ParseError, fmt};

/// The serialization format used for tracing events.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    /// Human-readable output intended for local development.
    #[default]
    Pretty,
    /// Newline-delimited structured JSON intended for production collection.
    Json,
}

/// Validated inputs used to initialize application tracing.
#[derive(Debug, Clone, Deserialize, Validate, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ObservabilityConfig {
    #[garde(custom(valid_filter))]
    filter: String,
    #[garde(skip)]
    format: LogFormat,
}

impl ObservabilityConfig {
    /// Creates tracing configuration from a filter directive and output format.
    #[must_use]
    pub fn new(filter: impl Into<String>, format: LogFormat) -> Self {
        Self {
            filter: filter.into(),
            format,
        }
    }

    /// Returns the tracing filter directive.
    #[must_use]
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Returns the configured output format.
    #[must_use]
    pub const fn format(&self) -> LogFormat {
        self.format
    }

    /// Validates the tracing filter without installing a global subscriber.
    ///
    /// # Errors
    ///
    /// Returns [`ObservabilityError::InvalidFilter`] when the directive cannot
    /// be parsed by `tracing-subscriber`.
    pub fn validate(&self) -> Result<(), ObservabilityError> {
        self.parsed_filter().map(|_| ())
    }

    fn parsed_filter(&self) -> Result<EnvFilter, ObservabilityError> {
        EnvFilter::try_new(&self.filter).map_err(ObservabilityError::InvalidFilter)
    }
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self::new("info", LogFormat::Pretty)
    }
}

/// A tracing configuration or initialization failure.
#[derive(Debug, Error)]
pub enum ObservabilityError {
    /// The configured filter directive is invalid.
    #[error("invalid tracing filter: {0}")]
    InvalidFilter(#[source] ParseError),

    /// A global tracing subscriber was already installed.
    #[error("tracing subscriber is already initialized")]
    AlreadyInitialized(#[source] tracing::subscriber::SetGlobalDefaultError),
}

fn valid_filter<Context>(value: &str, _context: &Context) -> garde::Result {
    EnvFilter::try_new(value)
        .map(|_| ())
        .map_err(|error| garde::Error::new(format!("invalid tracing filter: {error}")))
}

/// Installs the process-global tracing subscriber.
///
/// Log output is written to stderr. ANSI styling is enabled only for pretty
/// output attached to a terminal and is never used for JSON.
///
/// # Errors
///
/// Returns an error for an invalid filter or when another global subscriber has
/// already been installed.
pub fn init(config: &ObservabilityConfig) -> Result<(), ObservabilityError> {
    let filter = config.parsed_filter()?;
    match config.format {
        LogFormat::Pretty => {
            let subscriber = fmt()
                .with_env_filter(filter)
                .with_ansi(io::stderr().is_terminal())
                .with_writer(io::stderr)
                .compact()
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .map_err(ObservabilityError::AlreadyInitialized)
        }
        LogFormat::Json => {
            let subscriber = fmt()
                .with_env_filter(filter)
                .with_ansi(false)
                .with_writer(io::stderr)
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .map_err(ObservabilityError::AlreadyInitialized)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, Write},
        sync::{Arc, Mutex},
    };

    use serde_json::Value;
    use tracing::{info, info_span};
    use tracing_subscriber::{EnvFilter, fmt};

    use super::LogFormat;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().expect("capture lock").clone())
                .expect("tracing output should be UTF-8")
        }
    }

    impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for Captured {
        type Writer = CapturedWriter;

        fn make_writer(&'writer self) -> Self::Writer {
            CapturedWriter(Arc::clone(&self.0))
        }
    }

    struct CapturedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for CapturedWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("capture lock").write(buffer)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn emit(format: LogFormat) -> String {
        let capture = Captured::default();
        let filter = EnvFilter::try_new("info").expect("valid filter");
        match format {
            LogFormat::Pretty => {
                let subscriber = fmt()
                    .with_env_filter(filter)
                    .with_ansi(false)
                    .with_writer(capture.clone())
                    .compact()
                    .finish();
                tracing::subscriber::with_default(subscriber, emit_test_event);
            }
            LogFormat::Json => {
                let subscriber = fmt()
                    .with_env_filter(filter)
                    .with_ansi(false)
                    .with_writer(capture.clone())
                    .json()
                    .with_current_span(true)
                    .with_span_list(true)
                    .finish();
                tracing::subscriber::with_default(subscriber, emit_test_event);
            }
        }
        capture.text()
    }

    fn emit_test_event() {
        let span = info_span!("request", request_id = "req-123");
        let _guard = span.enter();
        info!(item_id = 7, "item created");
    }

    #[test]
    fn pretty_output_contains_level_message_fields_and_span() {
        let output = emit(LogFormat::Pretty);

        assert!(output.contains("INFO"));
        assert!(output.contains("item created"));
        assert!(output.contains("item_id=7"));
        assert!(output.contains("request_id=\"req-123\""));
    }

    #[test]
    fn json_output_is_structured_and_contains_span_context() {
        let output = emit(LogFormat::Json);
        let event: Value = serde_json::from_str(output.trim()).expect("valid JSON event");

        assert_eq!(event["level"], "INFO");
        assert_eq!(event["fields"]["message"], "item created");
        assert_eq!(event["fields"]["item_id"], 7);
        assert_eq!(event["span"]["name"], "request");
        assert_eq!(event["span"]["request_id"], "req-123");
    }
}
