use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum_server::tls_rustls::RustlsConfig;
use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt,
    NewAccount, NewOrder, OrderStatus, RetryPolicy,
};
use thiserror::Error;
use tokio::time::sleep;
use webstack_core::config::Config;
use x509_parser::pem::parse_x509_pem;

use super::cloudflare::{CloudflareDns, CloudflareError, TxtRecord};

const DNS01_DIRECTORY: &str = "dns-01";
const STAGING_DIRECTORY: &str = "staging";
const PRODUCTION_DIRECTORY: &str = "production";
const ACCOUNT_FILE: &str = "account.json";
const CHALLENGE_PREFIX: &str = "_acme-challenge";
const PROPAGATION_DELAY: Duration = Duration::from_secs(15);
const ORDER_TIMEOUT: Duration = Duration::from_secs(120);
const RENEWAL_RETRY_DELAY: Duration = Duration::from_hours(1);
const MAX_RENEWAL_SLEEP: Duration = Duration::from_hours(12);

/// A typed ACME DNS-01 certificate failure.
#[derive(Debug, Error)]
pub enum Dns01Error {
    #[error("cannot access ACME state at {}", path.display())]
    Storage {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("stored ACME account at {} is unreadable", path.display())]
    StoredAccount {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("certificate at {} is unreadable: {reason}", path.display())]
    StoredCertificate { path: PathBuf, reason: String },
    #[error("CLOUDFLARE_API_TOKEN is required to request a DNS-01 certificate")]
    MissingToken,
    #[error("ACME request failed")]
    Acme(#[source] instant_acme::Error),
    #[error("Cloudflare DNS update failed")]
    Cloudflare(#[source] CloudflareError),
    #[error("ACME authorization for {domain} is {status}")]
    Authorization { domain: String, status: String },
    #[error("the ACME server offered no DNS-01 challenge for {domain}")]
    MissingChallenge { domain: String },
    #[error("ACME order finished in state {status}")]
    Order { status: String },
    #[error("issued certificate is unreadable: {reason}")]
    IssuedCertificate { reason: String },
    #[error("cannot load the certificate into rustls")]
    Rustls(#[source] io::Error),
}

/// The validity window of one certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Validity {
    not_before: SystemTime,
    not_after: SystemTime,
}

impl Validity {
    /// Returns when renewal should start: after two thirds of the lifetime.
    fn renew_at(self) -> SystemTime {
        let lifetime = self
            .not_after
            .duration_since(self.not_before)
            .unwrap_or_default();
        self.not_after - lifetime / 3
    }
}

/// A PEM certificate chain, its private key, and the leaf validity window.
struct StoredCertificate {
    chain_pem: String,
    key_pem: String,
    validity: Validity,
}

/// Filesystem layout for one domain's DNS-01 account and certificate.
struct CertificateStore {
    directory: PathBuf,
    domain: String,
}

impl CertificateStore {
    /// Selects the cache directory for the configured ACME directory and domain.
    fn new(cache_dir: &Path, staging: bool, domain: &str) -> Self {
        let environment = if staging {
            STAGING_DIRECTORY
        } else {
            PRODUCTION_DIRECTORY
        };
        Self {
            directory: cache_dir.join(DNS01_DIRECTORY).join(environment),
            domain: domain.to_owned(),
        }
    }

    /// Returns the path of the persisted ACME account credentials.
    fn account_path(&self) -> PathBuf {
        self.directory.join(ACCOUNT_FILE)
    }

    /// Returns the path of the persisted certificate chain.
    fn chain_path(&self) -> PathBuf {
        self.directory.join(format!("{}.crt.pem", self.domain))
    }

    /// Returns the path of the persisted certificate private key.
    fn key_path(&self) -> PathBuf {
        self.directory.join(format!("{}.key.pem", self.domain))
    }

    /// Loads stored ACME account credentials when they exist.
    fn load_account(&self) -> Result<Option<AccountCredentials>, Dns01Error> {
        let path = self.account_path();
        let Some(source) = read_optional(&path)? else {
            return Ok(None);
        };
        serde_json::from_str(&source)
            .map(Some)
            .map_err(|source| Dns01Error::StoredAccount { path, source })
    }

    /// Persists ACME account credentials with owner-only permissions.
    fn save_account(&self, credentials: &AccountCredentials) -> Result<(), Dns01Error> {
        let path = self.account_path();
        let source =
            serde_json::to_string(credentials).map_err(|source| Dns01Error::StoredAccount {
                path: path.clone(),
                source,
            })?;
        write_private(&path, &source)
    }

    /// Loads the stored certificate and key when both exist.
    fn load_certificate(&self) -> Result<Option<StoredCertificate>, Dns01Error> {
        let chain_path = self.chain_path();
        let (Some(chain_pem), Some(key_pem)) = (
            read_optional(&chain_path)?,
            read_optional(&self.key_path())?,
        ) else {
            return Ok(None);
        };
        let validity =
            leaf_validity(&chain_pem).map_err(|reason| Dns01Error::StoredCertificate {
                path: chain_path,
                reason,
            })?;
        Ok(Some(StoredCertificate {
            chain_pem,
            key_pem,
            validity,
        }))
    }

    /// Persists a certificate and key, writing the key first.
    fn save_certificate(&self, certificate: &StoredCertificate) -> Result<(), Dns01Error> {
        write_private(&self.key_path(), &certificate.key_pem)?;
        write_private(&self.chain_path(), &certificate.chain_pem)
    }
}

/// Obtains certificates from an ACME directory using Cloudflare DNS-01 challenges.
struct Dns01Issuer {
    store: CertificateStore,
    domain: String,
    contact: String,
    directory_url: String,
    dns: Option<CloudflareDns>,
    propagation_delay: Duration,
}

impl Dns01Issuer {
    /// Builds an issuer from validated ACME configuration.
    fn from_config(config: &Config) -> Self {
        let domain = config.tls.domain.clone().expect("validated ACME domain");
        let email = config
            .tls
            .acme_email
            .as_deref()
            .expect("validated ACME email");
        let directory = if config.tls.acme_staging {
            LetsEncrypt::Staging
        } else {
            LetsEncrypt::Production
        };
        Self {
            store: CertificateStore::new(
                &config.tls.acme_cache_dir,
                config.tls.acme_staging,
                &domain,
            ),
            domain,
            contact: format!("mailto:{email}"),
            directory_url: directory.url().to_owned(),
            dns: config
                .tls
                .cloudflare_api_token()
                .cloned()
                .map(CloudflareDns::new),
            propagation_delay: PROPAGATION_DELAY,
        }
    }

    /// Orders a new certificate, persists it, and returns it.
    async fn issue_and_store(&self) -> Result<StoredCertificate, Dns01Error> {
        let dns = self.dns.as_ref().ok_or(Dns01Error::MissingToken)?;
        tracing::info!(domain = %self.domain, "requesting ACME DNS-01 certificate");
        let account = self.account().await?;
        let mut order = account
            .new_order(&NewOrder::new(&[Identifier::Dns(self.domain.clone())]))
            .await
            .map_err(Dns01Error::Acme)?;
        let mut records = Vec::new();
        let result = self.complete_order(&mut order, dns, &mut records).await;
        for record in &records {
            if let Err(error) = dns.delete_txt(record).await {
                tracing::warn!(error = %error, "cannot delete ACME challenge TXT record");
            }
        }
        let certificate = result?;
        self.store.save_certificate(&certificate)?;
        tracing::info!(
            domain = %self.domain,
            expires = %unix_seconds(certificate.validity.not_after),
            "ACME DNS-01 certificate issued"
        );
        Ok(certificate)
    }

    /// Restores the persisted ACME account or registers and persists a new one.
    async fn account(&self) -> Result<Account, Dns01Error> {
        if let Some(credentials) = self.store.load_account()? {
            return Account::builder()
                .map_err(Dns01Error::Acme)?
                .from_credentials(credentials)
                .await
                .map_err(Dns01Error::Acme);
        }
        let (account, credentials) = Account::builder()
            .map_err(Dns01Error::Acme)?
            .create(
                &NewAccount {
                    contact: &[self.contact.as_str()],
                    terms_of_service_agreed: true,
                    only_return_existing: false,
                },
                self.directory_url.clone(),
                None,
            )
            .await
            .map_err(Dns01Error::Acme)?;
        self.store.save_account(&credentials)?;
        tracing::info!("registered ACME account");
        Ok(account)
    }

    /// Publishes challenge records, waits for validation, and downloads the certificate.
    async fn complete_order(
        &self,
        order: &mut instant_acme::Order,
        dns: &CloudflareDns,
        records: &mut Vec<TxtRecord>,
    ) -> Result<StoredCertificate, Dns01Error> {
        let mut authorizations = order.authorizations();
        while let Some(authorization) = authorizations.next().await {
            let mut authorization = authorization.map_err(Dns01Error::Acme)?;
            match authorization.status {
                AuthorizationStatus::Pending => {}
                AuthorizationStatus::Valid => continue,
                status => {
                    return Err(Dns01Error::Authorization {
                        domain: self.domain.clone(),
                        status: format!("{status:?}"),
                    });
                }
            }
            let mut challenge = authorization
                .challenge(ChallengeType::Dns01)
                .ok_or_else(|| Dns01Error::MissingChallenge {
                    domain: self.domain.clone(),
                })?;
            let name = format!("{CHALLENGE_PREFIX}.{}", challenge.identifier());
            let value = challenge.key_authorization().dns_value();
            records.push(
                dns.create_txt(&name, &value)
                    .await
                    .map_err(Dns01Error::Cloudflare)?,
            );
            tracing::info!(record = %name, "published ACME challenge TXT record");
            sleep(self.propagation_delay).await;
            challenge.set_ready().await.map_err(Dns01Error::Acme)?;
        }
        let retries = RetryPolicy::new().timeout(ORDER_TIMEOUT);
        let status = order.poll_ready(&retries).await.map_err(Dns01Error::Acme)?;
        if status != OrderStatus::Ready {
            return Err(Dns01Error::Order {
                status: format!("{status:?}"),
            });
        }
        let key_pem = order.finalize().await.map_err(Dns01Error::Acme)?;
        let chain_pem = order
            .poll_certificate(&retries)
            .await
            .map_err(Dns01Error::Acme)?;
        let validity =
            leaf_validity(&chain_pem).map_err(|reason| Dns01Error::IssuedCertificate { reason })?;
        Ok(StoredCertificate {
            chain_pem,
            key_pem,
            validity,
        })
    }
}

/// A live DNS-01 certificate and the state needed to renew it.
pub(super) struct Dns01Runtime {
    pub(super) rustls_config: RustlsConfig,
    issuer: Dns01Issuer,
    renew_at: SystemTime,
}

impl Dns01Runtime {
    /// Renews the certificate before expiry and reloads it into the running listener.
    pub(super) async fn renew_forever(mut self) -> Result<(), super::TlsError> {
        loop {
            let wait = self
                .renew_at
                .duration_since(SystemTime::now())
                .unwrap_or_default();
            if !wait.is_zero() {
                sleep(wait.min(MAX_RENEWAL_SLEEP)).await;
                continue;
            }
            match self.renew().await {
                Ok(renew_at) => self.renew_at = renew_at,
                Err(error) => {
                    tracing::error!(error = %error_chain(&error), "ACME DNS-01 renewal failed");
                    self.renew_at = SystemTime::now() + RENEWAL_RETRY_DELAY;
                }
            }
        }
    }

    /// Issues and installs one replacement certificate, returning its renewal time.
    async fn renew(&self) -> Result<SystemTime, Dns01Error> {
        let certificate = self.issuer.issue_and_store().await?;
        self.rustls_config
            .reload_from_pem(
                certificate.chain_pem.into_bytes(),
                certificate.key_pem.into_bytes(),
            )
            .await
            .map_err(Dns01Error::Rustls)?;
        Ok(certificate.validity.renew_at())
    }
}

/// Loads a usable cached certificate or obtains one before the listener starts.
pub(super) async fn prepare(config: &Config) -> Result<Dns01Runtime, Dns01Error> {
    prepare_with(Dns01Issuer::from_config(config)).await
}

/// Selects the certificate to serve at startup for one issuer.
async fn prepare_with(issuer: Dns01Issuer) -> Result<Dns01Runtime, Dns01Error> {
    let now = SystemTime::now();
    let certificate = match issuer.store.load_certificate() {
        Ok(Some(cached)) if now < cached.validity.renew_at() => {
            tracing::info!(domain = %issuer.domain, "using cached ACME DNS-01 certificate");
            cached
        }
        Ok(Some(cached)) if now < cached.validity.not_after => {
            match issuer.issue_and_store().await {
                Ok(renewed) => renewed,
                Err(error) => {
                    tracing::warn!(
                        error = %error_chain(&error),
                        "ACME DNS-01 renewal failed; serving the cached certificate until it expires"
                    );
                    cached
                }
            }
        }
        Ok(_) => issuer.issue_and_store().await?,
        Err(error) => {
            tracing::warn!(error = %error_chain(&error), "ignoring unreadable cached certificate");
            issuer.issue_and_store().await?
        }
    };
    let renew_at = certificate.validity.renew_at();
    let rustls_config = RustlsConfig::from_pem(
        certificate.chain_pem.into_bytes(),
        certificate.key_pem.into_bytes(),
    )
    .await
    .map_err(Dns01Error::Rustls)?;
    Ok(Dns01Runtime {
        rustls_config,
        issuer,
        renew_at,
    })
}

/// Reads the validity window of the first certificate in a PEM chain.
fn leaf_validity(chain_pem: &str) -> Result<Validity, String> {
    let (_rest, pem) = parse_x509_pem(chain_pem.as_bytes()).map_err(|error| error.to_string())?;
    let certificate = pem.parse_x509().map_err(|error| error.to_string())?;
    let validity = certificate.validity();
    Ok(Validity {
        not_before: system_time(validity.not_before.timestamp())?,
        not_after: system_time(validity.not_after.timestamp())?,
    })
}

/// Converts Unix seconds from a certificate into a system time.
fn system_time(seconds: i64) -> Result<SystemTime, String> {
    u64::try_from(seconds)
        .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds))
        .map_err(|_| "certificate validity predates the Unix epoch".to_owned())
}

/// Formats a system time as Unix seconds for structured logs.
fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Reads a file, returning `None` when it does not exist.
fn read_optional(path: &Path) -> Result<Option<String>, Dns01Error> {
    match fs::read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Dns01Error::Storage {
            path: path.to_owned(),
            source,
        }),
    }
}

/// Atomically writes a secret file readable only by its owner.
fn write_private(path: &Path, contents: &str) -> Result<(), Dns01Error> {
    let storage_error = |source| Dns01Error::Storage {
        path: path.to_owned(),
        source,
    };
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(directory).map_err(storage_error)?;
    let temporary = path.with_extension("tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&temporary).map_err(storage_error)?;
    io::Write::write_all(&mut file, contents.as_bytes()).map_err(storage_error)?;
    file.sync_all().map_err(storage_error)?;
    fs::rename(&temporary, path).map_err(storage_error)
}

/// Formats an error with its source chain for one log field.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use rcgen::{CertificateParams, KeyPair};
    use secrecy::SecretString;
    use tempfile::tempdir;
    use time::OffsetDateTime;

    use super::{
        CertificateStore, CloudflareDns, Dns01Error, Dns01Issuer, StoredCertificate, Validity,
        leaf_validity, prepare_with,
    };

    const DAY: Duration = Duration::from_hours(24);

    fn offline_issuer(cache_dir: &Path, domain: &str, token: Option<SecretString>) -> Dns01Issuer {
        Dns01Issuer {
            store: CertificateStore::new(cache_dir, true, domain),
            domain: domain.to_owned(),
            contact: "mailto:admin@example.com".to_owned(),
            directory_url: "http://127.0.0.1:9/directory".to_owned(),
            dns: token
                .map(|token| CloudflareDns::with_api_base(token, "http://127.0.0.1:9".to_owned())),
            propagation_delay: Duration::ZERO,
        }
    }

    fn certificate(
        domain: &str,
        not_before: SystemTime,
        not_after: SystemTime,
    ) -> StoredCertificate {
        let mut params =
            CertificateParams::new(vec![domain.to_owned()]).expect("certificate params");
        params.not_before = OffsetDateTime::from(not_before);
        params.not_after = OffsetDateTime::from(not_after);
        let key = KeyPair::generate().expect("key pair");
        let cert = params.self_signed(&key).expect("self-signed certificate");
        StoredCertificate {
            chain_pem: cert.pem(),
            key_pem: key.serialize_pem(),
            validity: Validity {
                not_before,
                not_after,
            },
        }
    }

    fn whole_seconds(time: SystemTime) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(time.duration_since(UNIX_EPOCH).expect("epoch").as_secs())
    }

    #[test]
    fn renewal_starts_after_two_thirds_of_the_lifetime() {
        let not_before = UNIX_EPOCH + 1000 * DAY;
        let ninety_days = Validity {
            not_before,
            not_after: not_before + 90 * DAY,
        };
        assert_eq!(ninety_days.renew_at(), not_before + 60 * DAY);

        let six_days = Validity {
            not_before,
            not_after: not_before + 6 * DAY,
        };
        assert_eq!(six_days.renew_at(), not_before + 4 * DAY);
    }

    #[test]
    fn certificates_round_trip_with_their_validity_and_private_permissions() {
        let cache = tempdir().expect("cache directory");
        let store = CertificateStore::new(cache.path(), false, "app.example.com");
        let now = whole_seconds(SystemTime::now());
        let issued = certificate("app.example.com", now - DAY, now + 89 * DAY);

        assert!(store.load_certificate().expect("empty store").is_none());
        store.save_certificate(&issued).expect("saved certificate");
        let loaded = store
            .load_certificate()
            .expect("readable store")
            .expect("stored certificate");

        assert_eq!(loaded.validity, issued.validity);
        assert_eq!(loaded.key_pem, issued.key_pem);
        assert!(
            store
                .chain_path()
                .starts_with(cache.path().join("dns-01/production"))
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(store.key_path())
                .expect("key metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert_eq!(
            leaf_validity(&issued.chain_pem).expect("leaf validity"),
            issued.validity
        );
    }

    #[tokio::test]
    async fn a_fresh_cached_certificate_is_served_without_contacting_anyone() {
        let cache = tempdir().expect("cache directory");
        let now = whole_seconds(SystemTime::now());
        let issuer = offline_issuer(cache.path(), "app.example.com", None);
        issuer
            .store
            .save_certificate(&certificate("app.example.com", now - DAY, now + 89 * DAY))
            .expect("cached certificate");

        let runtime = prepare_with(issuer)
            .await
            .expect("cached certificate runtime");

        assert_eq!(runtime.renew_at, now + 59 * DAY);
    }

    #[tokio::test]
    async fn a_due_certificate_is_still_served_when_renewal_fails() {
        let cache = tempdir().expect("cache directory");
        let now = whole_seconds(SystemTime::now());
        let issuer = offline_issuer(
            cache.path(),
            "app.example.com",
            Some(SecretString::from("token".to_owned())),
        );
        issuer
            .store
            .save_certificate(&certificate(
                "app.example.com",
                now - 80 * DAY,
                now + 10 * DAY,
            ))
            .expect("cached certificate");

        let runtime = prepare_with(issuer).await.expect("cached fallback runtime");

        assert!(runtime.renew_at <= SystemTime::now());
    }

    #[tokio::test]
    async fn startup_fails_without_a_usable_certificate_or_token() {
        let cache = tempdir().expect("cache directory");
        let now = whole_seconds(SystemTime::now());
        let issuer = offline_issuer(cache.path(), "app.example.com", None);
        issuer
            .store
            .save_certificate(&certificate("app.example.com", now - 90 * DAY, now - DAY))
            .expect("expired certificate");

        let error = prepare_with(issuer).await.err().expect("startup failure");

        assert!(matches!(error, Dns01Error::MissingToken));
    }
}
