use webstack_core::observability::{self, LogFormat, ObservabilityConfig, ObservabilityError};

#[test]
fn observability_defaults_to_pretty_info_logs() {
    let config = ObservabilityConfig::default();

    assert_eq!(config.filter(), "info");
    assert_eq!(config.format(), LogFormat::Pretty);
    config.validate().expect("default filter should be valid");
}

#[test]
fn invalid_filters_return_a_typed_error() {
    let config = ObservabilityConfig::new("not a [ valid filter", LogFormat::Json);

    let error = config.validate().expect_err("filter should be rejected");
    assert!(error.to_string().contains("invalid tracing filter"));
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn initializing_a_second_global_subscriber_returns_a_typed_error() {
    let config = ObservabilityConfig::new("off", LogFormat::Pretty);
    observability::init(&config).expect("first subscriber should initialize");

    let error = observability::init(&config).expect_err("second subscriber should fail");
    assert!(matches!(error, ObservabilityError::AlreadyInitialized(_)));
    assert!(std::error::Error::source(&error).is_some());
}
