//! Typed TOML configuration loading and validation.

use std::{
    env, fmt, fs, io,
    net::IpAddr,
    path::{Path, PathBuf},
};

use garde::Validate;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use thiserror::Error;

pub use crate::observability::{LogFormat, ObservabilityConfig};

const CONFIG_PATH: &str = "./webstack.toml";

/// Complete Webstack-owned configuration.
#[derive(Debug, Default, Deserialize, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    #[garde(skip)]
    pub environment: Environment,
    #[garde(dive)]
    pub assets: AssetsConfig,
    #[garde(dive)]
    pub server: ServerConfig,
    #[garde(dive)]
    pub tls: TlsConfig,
    #[garde(dive)]
    pub database: DatabaseConfig,
    #[garde(dive)]
    pub auth: AuthConfig,
    #[garde(dive)]
    pub backup: BackupConfig,
    #[garde(dive)]
    pub observability: ObservabilityConfig,
}

impl Config {
    /// Loads Webstack configuration from `./webstack.toml`.
    ///
    /// # Errors
    ///
    /// Returns a typed error for environment overrides, filesystem access,
    /// TOML parsing, or validation.
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_file(Path::new(CONFIG_PATH))
    }

    /// Loads, parses, overrides, and validates configuration from one path.
    fn load_file(path: &Path) -> Result<Self, ConfigError> {
        let source = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_owned(),
            source,
        })?;
        let mut config: Self = toml::from_str(&source).map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source,
        })?;
        config.apply_environment()?;
        config.validate()?;
        Ok(config)
    }

    /// Reads the supported secret environment overrides.
    fn apply_environment(&mut self) -> Result<(), ConfigError> {
        self.apply_environment_values(
            environment_value("R2_ACCOUNT_ID")?,
            environment_value("R2_ACCESS_KEY_ID")?,
            environment_value("R2_SECRET_ACCESS_KEY")?,
        );
        Ok(())
    }

    /// Applies supplied R2 values after TOML deserialization.
    fn apply_environment_values(
        &mut self,
        account_id: Option<String>,
        access_key_id: Option<String>,
        secret_access_key: Option<String>,
    ) {
        if let Some(value) = account_id {
            self.backup.r2.account_id = value;
        }
        if access_key_id.is_some() || secret_access_key.is_some() {
            self.backup.r2.credentials = Some(R2Credentials {
                access_key_id: SecretString::from(access_key_id.unwrap_or_default()),
                secret_access_key: SecretString::from(secret_access_key.unwrap_or_default()),
            });
        }
    }

    /// Aggregates validation failures into Webstack's typed configuration error.
    fn validate(&self) -> Result<(), ConfigError> {
        let mut issues = Validate::validate(self)
            .err()
            .map(|report| {
                report
                    .into_inner()
                    .into_iter()
                    .map(|(path, error)| ValidationIssue {
                        field: path.to_string(),
                        message: error.to_string(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if self.tls.mode != TlsMode::Disabled
            && self.server.http_redirect
            && self.server.http_port == self.server.https_port
        {
            issues.push(ValidationIssue {
                field: "server.http_port".to_owned(),
                message: "must differ from server.https_port when HTTP redirects are enabled"
                    .to_owned(),
            });
        }
        if issues.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Validation { issues })
        }
    }
}

/// The runtime environment controlling safe development conveniences.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    Development,
    #[default]
    Production,
}

/// Versions of frontend artifacts managed by Webstack tooling.
#[derive(Debug, Deserialize, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct AssetsConfig {
    #[garde(custom(semantic_version))]
    pub tailwind_version: String,
    #[garde(custom(semantic_version))]
    pub daisyui_version: String,
    #[garde(custom(semantic_version))]
    pub htmx_version: String,
}

impl Default for AssetsConfig {
    /// Returns the frontend versions shipped by newly generated applications.
    fn default() -> Self {
        Self {
            tailwind_version: "4.3.1".to_owned(),
            daisyui_version: "5.6.18".to_owned(),
            htmx_version: "4.0.0-beta5".to_owned(),
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    #[garde(skip)]
    pub bind_addr: IpAddr,
    #[garde(range(min = 1))]
    pub https_port: u16,
    #[garde(range(min = 1))]
    pub http_port: u16,
    #[garde(skip)]
    pub http_redirect: bool,
    #[garde(range(min = 1, max = 300))]
    pub shutdown_timeout_seconds: u64,
}

impl Default for ServerConfig {
    /// Returns local, unprivileged server defaults.
    fn default() -> Self {
        Self {
            bind_addr: IpAddr::from([127, 0, 0, 1]),
            https_port: 8443,
            http_port: 8080,
            http_redirect: false,
            shutdown_timeout_seconds: 30,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    Acme,
    SelfSigned,
    #[default]
    Disabled,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct TlsConfig {
    #[garde(skip)]
    pub mode: TlsMode,
    #[garde(if(cond = self.mode == TlsMode::Acme, required, inner(custom(acme_domain))))]
    pub domain: Option<String>,
    #[garde(if(cond = self.mode == TlsMode::Acme, required, inner(custom(email_address))))]
    pub acme_email: Option<String>,
    #[garde(if(cond = self.mode == TlsMode::Acme, custom(nonempty_path)))]
    pub acme_cache_dir: PathBuf,
    #[garde(skip)]
    pub acme_staging: bool,
}

impl Default for TlsConfig {
    /// Returns disabled TLS defaults suitable for initial local development.
    fn default() -> Self {
        Self {
            mode: TlsMode::Disabled,
            domain: None,
            acme_email: None,
            acme_cache_dir: PathBuf::from("./data/acme"),
            acme_staging: false,
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseConfig {
    #[garde(custom(nonempty_path))]
    pub data_dir: PathBuf,
    #[garde(custom(non_blank))]
    pub namespace: String,
    #[garde(custom(non_blank))]
    pub database: String,
}

impl Default for DatabaseConfig {
    /// Returns the conventional embedded database location and identifiers.
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("./data/surreal"),
            namespace: "app".to_owned(),
            database: "app".to_owned(),
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct AuthConfig {
    #[garde(range(min = 1))]
    pub session_ttl_hours: u64,
    #[garde(skip)]
    pub bootstrap_admin: bool,
    #[garde(range(min = 1))]
    pub password_ttl_days: u16,
}

impl Default for AuthConfig {
    /// Returns one-week sessions with first-run admin bootstrapping enabled.
    fn default() -> Self {
        Self {
            session_ttl_hours: 168,
            bootstrap_admin: true,
            password_ttl_days: 90,
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct BackupConfig {
    #[garde(skip)]
    pub enabled: bool,
    #[garde(if(cond = self.enabled, custom(non_blank)))]
    pub cron: String,
    #[garde(if(cond = self.enabled, range(min = 1)))]
    pub retention: usize,
    #[garde(dive(self.enabled))]
    pub r2: R2Config,
}

impl Default for BackupConfig {
    /// Returns disabled backup defaults with the standard daily schedule.
    fn default() -> Self {
        Self {
            enabled: false,
            cron: "0 0 3 * * *".to_owned(),
            retention: 14,
            r2: R2Config::default(),
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
#[serde(default, deny_unknown_fields)]
#[garde(context(bool as enabled))]
pub struct R2Config {
    #[garde(if(cond = *enabled, custom(non_blank)))]
    pub account_id: String,
    #[garde(if(cond = *enabled, custom(non_blank)))]
    pub bucket: String,
    #[garde(skip)]
    pub prefix: String,
    #[serde(skip)]
    #[garde(if(cond = *enabled, required), dive(*enabled))]
    credentials: Option<R2Credentials>,
}

impl R2Config {
    /// Returns the environment-supplied R2 credentials when present.
    #[must_use]
    pub const fn credentials(&self) -> Option<&R2Credentials> {
        self.credentials.as_ref()
    }
}

impl Default for R2Config {
    /// Returns empty R2 settings with the standard backup key prefix.
    fn default() -> Self {
        Self {
            account_id: String::new(),
            bucket: String::new(),
            prefix: "db-backups/".to_owned(),
            credentials: None,
        }
    }
}

#[derive(Debug, Validate)]
#[garde(context(bool as enabled))]
pub struct R2Credentials {
    #[garde(if(cond = *enabled, custom(non_blank_secret)))]
    pub access_key_id: SecretString,
    #[garde(if(cond = *enabled, custom(non_blank_secret)))]
    pub secret_access_key: SecretString,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    field: String,
    message: String,
}

impl ValidationIssue {
    /// Returns the configuration field associated with this issue.
    #[must_use]
    pub fn field(&self) -> &str {
        &self.field
    }

    /// Returns the validation message for this issue.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ValidationIssue {
    /// Formats one validation issue as its field and message.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.field, self.message)
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("environment variable {0} is not valid Unicode")]
    NonUnicodeEnvironment(&'static str),
    #[error("cannot read configuration {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot parse configuration {}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("configuration validation failed: {issues:?}")]
    Validation { issues: Vec<ValidationIssue> },
}

/// Validates a semantic-version string for Garde.
fn semantic_version<Context>(value: &str, _context: &Context) -> garde::Result {
    semver::Version::parse(value)
        .map(|_| ())
        .map_err(|_| garde::Error::new("must be a semantic version such as 4.3.1"))
}

/// Validates one DNS hostname accepted for an ACME certificate order.
fn acme_domain<Context>(value: &str, _context: &Context) -> garde::Result {
    let valid = value.len() <= 253
        && value.contains('.')
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
        });
    if valid {
        Ok(())
    } else {
        Err(garde::Error::new(
            "must be a DNS hostname such as app.example.com",
        ))
    }
}

/// Validates one basic ACME account contact email address.
fn email_address<Context>(value: &str, _context: &Context) -> garde::Result {
    let mut parts = value.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    if !local.is_empty()
        && !domain.is_empty()
        && parts.next().is_none()
        && !value.chars().any(char::is_whitespace)
    {
        Ok(())
    } else {
        Err(garde::Error::new(
            "must be an email address such as admin@example.com",
        ))
    }
}

/// Reads one Unicode environment variable as an optional value.
fn environment_value(name: &'static str) -> Result<Option<String>, ConfigError> {
    env::var_os(name)
        .map(|value| {
            value
                .into_string()
                .map_err(|_| ConfigError::NonUnicodeEnvironment(name))
        })
        .transpose()
}

/// Rejects blank configuration strings for Garde.
fn non_blank<Context>(value: &str, _context: &Context) -> garde::Result {
    if value.trim().is_empty() {
        Err(garde::Error::new("must not be empty"))
    } else {
        Ok(())
    }
}

/// Rejects empty configuration paths for Garde.
fn nonempty_path<Context>(value: &Path, _context: &Context) -> garde::Result {
    if value.as_os_str().is_empty() {
        Err(garde::Error::new("must not be empty"))
    } else {
        Ok(())
    }
}

/// Rejects blank secret values without exposing them in diagnostics.
fn non_blank_secret<Context>(value: &SecretString, _context: &Context) -> garde::Result {
    if value.expose_secret().trim().is_empty() {
        Err(garde::Error::new("must not be empty"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        net::IpAddr,
        path::{Path, PathBuf},
    };

    use secrecy::{ExposeSecret, SecretString};
    use tempfile::TempDir;

    use super::{Config, ConfigError, Environment, LogFormat, R2Credentials, TlsMode};

    fn write_config(directory: &Path, contents: &str) -> std::path::PathBuf {
        let path = directory.join("webstack.toml");
        fs::write(&path, contents).expect("write test configuration");
        path
    }

    fn validation_fields(error: ConfigError) -> Vec<String> {
        let ConfigError::Validation { issues } = error else {
            panic!("expected validation error: {error}");
        };
        issues
            .into_iter()
            .map(|issue| issue.field().to_owned())
            .collect()
    }

    #[test]
    fn empty_document_uses_safe_defaults() {
        let temp = TempDir::new().expect("temporary directory");
        let path = write_config(temp.path(), "");

        let config = Config::load_file(&path).expect("valid defaults");

        assert_eq!(config.server.bind_addr, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(config.environment, Environment::Production);
        assert_eq!(config.server.http_port, 8080);
        assert_eq!(config.server.shutdown_timeout_seconds, 30);
        assert_eq!(config.tls.mode, TlsMode::Disabled);
        assert!(!config.backup.enabled);
        assert_eq!(config.observability.format(), LogFormat::Pretty);
    }

    #[test]
    fn complete_framework_document_parses() {
        let temp = TempDir::new().expect("temporary directory");
        let path = write_config(
            temp.path(),
            r#"
[server]
bind_addr = "0.0.0.0"
http_port = 9000

[observability]
filter = "webstack=debug"
format = "json"
"#,
        );

        let config = Config::load_file(&path).expect("valid config");

        assert_eq!(config.server.http_port, 9000);
        assert_eq!(config.assets.tailwind_version, "4.3.1");
        assert_eq!(config.observability.format(), LogFormat::Json);
    }

    #[test]
    fn asset_versions_must_be_semantic_versions() {
        let temp = TempDir::new().expect("temporary directory");
        let path = write_config(temp.path(), "[assets]\ntailwind_version = \"latest\"\n");

        let fields = validation_fields(Config::load_file(&path).expect_err("invalid version"));
        assert!(
            fields
                .iter()
                .any(|field| field == "assets.tailwind_version")
        );
    }

    #[test]
    fn unknown_framework_fields_are_rejected() {
        let temp = TempDir::new().expect("temporary directory");
        let path = write_config(temp.path(), "[server]\nunknown = true\n");

        let error = Config::load_file(&path).expect_err("unknown field");

        assert!(matches!(error, ConfigError::Parse { .. }));
    }

    #[test]
    fn application_section_is_not_part_of_webstack_configuration() {
        let temp = TempDir::new().expect("temporary directory");
        let path = write_config(temp.path(), "[application]\ntitle = \"Inventory\"\n");

        let error = Config::load_file(&path).expect_err("application config rejected");

        assert!(matches!(error, ConfigError::Parse { .. }));
    }

    #[test]
    fn credentials_in_toml_are_rejected() {
        let temp = TempDir::new().expect("temporary directory");
        let path = write_config(
            temp.path(),
            "[backup.r2]\naccess_key_id = \"must-not-be-here\"\n",
        );

        let error = Config::load_file(&path).expect_err("TOML secret rejected");

        assert!(matches!(error, ConfigError::Parse { .. }));
    }

    #[test]
    fn missing_webstack_file_is_an_error() {
        let temp = TempDir::new().expect("temporary directory");
        let path = temp.path().join("webstack.toml");

        let error = Config::load_file(&path).expect_err("missing file");

        assert!(matches!(error, ConfigError::Read { .. }));
    }

    #[test]
    fn validation_reports_every_problem() {
        let temp = TempDir::new().expect("temporary directory");
        let path = write_config(
            temp.path(),
            r#"
[database]
namespace = ""
database = ""

[auth]
session_ttl_hours = 0
password_ttl_days = 0

[observability]
filter = "not a [ valid filter"
"#,
        );

        let error = Config::load_file(&path).expect_err("invalid config");
        let fields = validation_fields(error);

        assert!(fields.iter().any(|field| field == "database.namespace"));
        assert!(fields.iter().any(|field| field == "database.database"));
        assert!(fields.iter().any(|field| field == "auth.session_ttl_hours"));
        assert!(fields.iter().any(|field| field == "auth.password_ttl_days"));
        assert!(fields.iter().any(|field| field == "observability.filter"));
    }

    #[test]
    fn r2_overrides_replace_account_and_keep_credentials_redacted() {
        let mut config = Config::default();
        config.apply_environment_values(
            Some("account-from-env".to_owned()),
            Some("access-secret".to_owned()),
            Some("key-secret".to_owned()),
        );

        let credentials = config.backup.r2.credentials().expect("credentials");
        assert_eq!(config.backup.r2.account_id, "account-from-env");
        assert_eq!(credentials.access_key_id.expose_secret(), "access-secret");
        let debug = format!("{config:?}");
        assert!(!debug.contains("access-secret"));
        assert!(!debug.contains("key-secret"));
    }

    #[test]
    fn acme_validation_is_conditional_and_aggregated() {
        let mut config = Config::default();
        config.tls.mode = TlsMode::Acme;
        config.tls.domain = Some("  ".to_owned());
        config.tls.acme_email = None;
        config.tls.acme_cache_dir = PathBuf::new();

        let fields = validation_fields(config.validate().expect_err("invalid ACME config"));

        assert!(fields.iter().any(|field| field == "tls.domain"));
        assert!(fields.iter().any(|field| field == "tls.acme_email"));
        assert!(fields.iter().any(|field| field == "tls.acme_cache_dir"));
    }

    #[test]
    fn listener_ports_and_acme_identity_are_validated_before_binding() {
        let mut config = Config::default();
        config.server.http_port = 0;
        config.server.https_port = 0;
        config.server.http_redirect = true;
        config.tls.mode = TlsMode::Acme;
        config.tls.domain = Some("https://example.com/path".to_owned());
        config.tls.acme_email = Some("not-an-email".to_owned());

        let fields = validation_fields(config.validate().expect_err("invalid TLS listeners"));

        for expected in [
            "server.http_port",
            "server.https_port",
            "tls.domain",
            "tls.acme_email",
        ] {
            assert!(
                fields.iter().any(|field| field == expected),
                "missing {expected}"
            );
        }

        config.server.http_port = 8443;
        config.server.https_port = 8443;
        config.tls.domain = Some("app.example.com".to_owned());
        config.tls.acme_email = Some("admin@example.com".to_owned());
        let fields = validation_fields(config.validate().expect_err("colliding TLS listeners"));
        assert!(fields.iter().any(|field| field == "server.http_port"));
    }

    #[test]
    fn shutdown_timeout_is_bounded() {
        let mut config = Config::default();
        config.server.shutdown_timeout_seconds = 0;
        let fields = validation_fields(config.validate().expect_err("zero timeout"));
        assert!(
            fields
                .iter()
                .any(|field| field == "server.shutdown_timeout_seconds")
        );

        config.server.shutdown_timeout_seconds = 301;
        let fields = validation_fields(config.validate().expect_err("long timeout"));
        assert!(
            fields
                .iter()
                .any(|field| field == "server.shutdown_timeout_seconds")
        );
    }

    #[test]
    fn enabled_backup_validates_r2_and_redacted_credentials() {
        let mut config = Config::default();
        config.backup.enabled = true;
        config.backup.cron = " ".to_owned();
        config.backup.retention = 0;

        let fields = validation_fields(config.validate().expect_err("invalid backup config"));
        for expected in [
            "backup.cron",
            "backup.retention",
            "backup.r2.account_id",
            "backup.r2.bucket",
            "backup.r2.credentials",
        ] {
            assert!(
                fields.iter().any(|field| field == expected),
                "missing {expected}"
            );
        }

        config.backup.cron = "0 0 3 * * *".to_owned();
        config.backup.retention = 14;
        config.backup.r2.account_id = "account".to_owned();
        config.backup.r2.bucket = "bucket".to_owned();
        config.backup.r2.credentials = Some(R2Credentials {
            access_key_id: SecretString::from(String::new()),
            secret_access_key: SecretString::from(" ".to_owned()),
        });

        let error = config.validate().expect_err("blank credentials");
        let debug = format!("{error:?}");
        let fields = validation_fields(error);
        assert!(
            fields
                .iter()
                .any(|field| field == "backup.r2.credentials.access_key_id")
        );
        assert!(
            fields
                .iter()
                .any(|field| field == "backup.r2.credentials.secret_access_key")
        );
        assert!(!debug.contains("Secret("));
    }
}
