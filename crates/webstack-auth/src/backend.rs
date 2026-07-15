use std::{collections::HashSet, sync::OnceLock};

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum_login::{AuthnBackend, AuthzBackend};
use surrealdb::types::SurrealValue;
use time::{Duration, OffsetDateTime};
use webstack_core::config::{AuthConfig, Environment};
use webstack_db::Database;

use crate::{AuthError, user::User};

const BOOTSTRAP_USERNAME: &str = "admin";
const BOOTSTRAP_PASSWORD: &str = "changeme";
const DEVELOPMENT_EXPIRY: i64 = 32_535_129_600;

#[derive(SurrealValue)]
struct Count {
    count: usize,
}

/// Username and password submitted to the authentication backend.
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// The swappable `axum-login` backend over Webstack's shared database.
#[derive(Clone, Debug)]
pub struct AuthBackend {
    database: Database,
}

impl AuthBackend {
    /// Creates an authentication backend using the shared database handle.
    #[must_use]
    pub const fn new(database: Database) -> Self {
        Self { database }
    }

    /// Loads a user by normalized username.
    ///
    /// # Errors
    ///
    /// Returns an authentication database error when the query fails.
    pub async fn find_user(&self, username: &str) -> Result<Option<User>, AuthError> {
        let username = normalize_username(username);
        let mut response = self
            .database
            .query("SELECT username, password_hash, roles, disabled, created_at, password_expires_at FROM type::record('_webstack_user', $username);")
            .bind(("username", username))
            .await
            .and_then(surrealdb::IndexedResults::check)
            .map_err(|source| AuthError::Database(Box::new(source)))?;
        response
            .take::<Option<User>>(0)
            .map_err(|source| AuthError::Database(Box::new(source)))
    }

    /// Replaces a user's password and expiration timestamp.
    ///
    /// # Errors
    ///
    /// Returns an authentication database or schema error when replacement fails.
    pub async fn replace_password(
        &self,
        username: &str,
        password_hash: &str,
        password_ttl_days: u16,
    ) -> Result<User, AuthError> {
        let expires = OffsetDateTime::now_utc()
            .saturating_add(Duration::days(i64::from(password_ttl_days)))
            .unix_timestamp();
        let mut response = self
            .database
            .query("UPDATE type::record('_webstack_user', $username) SET password_hash = $password_hash, password_expires_at = $expires RETURN AFTER;")
            .bind(("username", normalize_username(username)))
            .bind(("password_hash", password_hash.to_owned()))
            .bind(("expires", expires))
            .await
            .and_then(surrealdb::IndexedResults::check)
            .map_err(|source| AuthError::Database(Box::new(source)))?;
        response
            .take::<Option<User>>(0)
            .map_err(|source| AuthError::Database(Box::new(source)))?
            .ok_or(AuthError::MissingSchema)
    }
}

impl AuthnBackend for AuthBackend {
    type User = User;
    type Credentials = Credentials;
    type Error = AuthError;

    /// Authenticates normalized local credentials without revealing failure reasons.
    async fn authenticate(&self, credentials: Credentials) -> Result<Option<User>, AuthError> {
        let user = self.find_user(&credentials.username).await?;
        let hash = user
            .as_ref()
            .map_or_else(dummy_password_hash, |user| user.password_hash.clone());
        let password = credentials.password;
        let valid = tokio::task::spawn_blocking(move || verify_password(&password, &hash))
            .await
            .map_err(AuthError::PasswordWorker)?;
        Ok(user.filter(|user| valid && !user.disabled))
    }

    /// Loads the current account by its stable ID.
    async fn get_user(&self, user_id: &String) -> Result<Option<User>, AuthError> {
        Ok(self.find_user(user_id).await?.filter(|user| !user.disabled))
    }
}

impl AuthzBackend for AuthBackend {
    type Permission = String;

    /// Treats assigned role names as the permissions checked by role guards.
    async fn get_user_permissions(&self, user: &User) -> Result<HashSet<String>, AuthError> {
        Ok(user.roles.iter().cloned().collect())
    }
}

/// Creates the initial administrator when configured and no accounts exist.
///
/// # Errors
///
/// Returns a database or password-hashing error when bootstrap cannot complete.
pub async fn bootstrap_admin(
    database: &Database,
    config: &AuthConfig,
    environment: Environment,
) -> Result<(), AuthError> {
    if !config.bootstrap_admin {
        return Ok(());
    }
    let mut response = database
        .query("SELECT count() AS count FROM _webstack_user GROUP ALL;")
        .await
        .and_then(surrealdb::IndexedResults::check)
        .map_err(|source| AuthError::Database(Box::new(source)))?;
    let counts: Vec<Count> = response
        .take(0)
        .map_err(|source| AuthError::Database(Box::new(source)))?;
    if counts.first().is_some_and(|count| count.count > 0) {
        return Ok(());
    }
    let password_hash = hash_password(BOOTSTRAP_PASSWORD).await?;
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let expires = match environment {
        Environment::Development => DEVELOPMENT_EXPIRY,
        Environment::Production => now,
    };
    database
        .query("CREATE ONLY type::record('_webstack_user', 'admin') CONTENT { username: 'admin', password_hash: $password_hash, roles: ['admin', 'user'], disabled: false, created_at: $now, password_expires_at: $expires };")
        .bind(("password_hash", password_hash))
        .bind(("now", now))
        .bind(("expires", expires))
        .await
        .and_then(surrealdb::IndexedResults::check)
        .map_err(|source| AuthError::Database(Box::new(source)))?;
    tracing::warn!(
        username = BOOTSTRAP_USERNAME,
        "bootstrap administrator created with the documented temporary password"
    );
    Ok(())
}

/// Hashes a password with Argon2id and a fresh salt.
///
/// # Errors
///
/// Returns a hashing or worker error when Argon2id cannot produce the hash.
pub async fn hash_password(password: &str) -> Result<String, AuthError> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|hash| hash.to_string())
            .map_err(|_| AuthError::PasswordHash)
    })
    .await
    .map_err(AuthError::PasswordWorker)?
}

/// Normalizes usernames for lookup and stable record IDs.
fn normalize_username(username: &str) -> String {
    username.trim().to_ascii_lowercase()
}

/// Verifies one password against a stored PHC string.
fn verify_password(password: &str, encoded: &str) -> bool {
    PasswordHash::new(encoded).ok().is_some_and(|hash| {
        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok()
    })
}

/// Returns a valid fixed hash used for unknown-user timing work.
fn dummy_password_hash() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        let salt = SaltString::encode_b64(b"webstack-auth-dummy-salt")
            .expect("static dummy salt is valid");
        Argon2::default()
            .hash_password(b"webstack-auth-dummy-password", &salt)
            .expect("static dummy password hashes")
            .to_string()
    })
    .clone()
}
