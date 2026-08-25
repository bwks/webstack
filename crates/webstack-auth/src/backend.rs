use std::{collections::HashSet, sync::OnceLock};

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum_login::{AuthnBackend, AuthzBackend};
use time::{Duration, OffsetDateTime};
use webstack_core::config::{AuthConfig, Environment};
use webstack_db::Database;

use crate::{AuthError, user::User};

const BOOTSTRAP_USERNAME: &str = "admin";
const BOOTSTRAP_PASSWORD: &str = "changeme";
const DEVELOPMENT_EXPIRY: i64 = 32_535_129_600;
const USER_COLUMNS: &str =
    "username, password_hash, roles, disabled, created_at, password_expires_at";

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
        let connection = auth_connection(&self.database).await?;
        let sql = format!("SELECT {USER_COLUMNS} FROM _webstack_user WHERE username = ?1");
        let mut rows = connection
            .query(sql, (normalize_username(username),))
            .await
            .map_err(database_error)?;
        rows.next()
            .await
            .map_err(database_error)?
            .map(|row| decode_user(&row))
            .transpose()
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
        let connection = auth_connection(&self.database).await?;
        let sql = format!(
            "UPDATE _webstack_user SET password_hash = ?1, password_expires_at = ?2 \
             WHERE username = ?3 RETURNING {USER_COLUMNS}"
        );
        let mut rows = connection
            .query(sql, (password_hash, expires, normalize_username(username)))
            .await
            .map_err(database_error)?;
        rows.next()
            .await
            .map_err(database_error)?
            .map(|row| decode_user(&row))
            .transpose()?
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
    let connection = auth_connection(database).await?;
    let mut rows = connection
        .query("SELECT COUNT(*) FROM _webstack_user", ())
        .await
        .map_err(database_error)?;
    let count = rows
        .next()
        .await
        .map_err(database_error)?
        .ok_or(AuthError::MissingSchema)?
        .get::<i64>(0)
        .map_err(database_error)?;
    if count > 0 {
        return Ok(());
    }
    let password_hash = hash_password(BOOTSTRAP_PASSWORD).await?;
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let expires = match environment {
        Environment::Development => DEVELOPMENT_EXPIRY,
        Environment::Production => now,
    };
    let roles = serde_json::to_string(&["admin", "user"])
        .map_err(|error| AuthError::DatabaseDecode(error.to_string()))?;
    connection
        .execute(
            "INSERT INTO _webstack_user \
             (username, password_hash, roles, disabled, created_at, password_expires_at) \
             VALUES (?1, ?2, ?3, 0, ?4, ?5)",
            (BOOTSTRAP_USERNAME, password_hash, roles, now, expires),
        )
        .await
        .map_err(database_error)?;
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

/// Opens one configured connection for an authentication operation.
async fn auth_connection(database: &Database) -> Result<turso::Connection, AuthError> {
    database.connection().await.map_err(database_error)
}

/// Decodes one database row into an authenticated user.
fn decode_user(row: &turso::Row) -> Result<User, AuthError> {
    let roles_json: String = row.get(2).map_err(database_error)?;
    let roles = serde_json::from_str(&roles_json)
        .map_err(|error| AuthError::DatabaseDecode(error.to_string()))?;
    Ok(User::from_database(
        row.get(0).map_err(database_error)?,
        row.get(1).map_err(database_error)?,
        roles,
        row.get::<i64>(3).map_err(database_error)? != 0,
        row.get(4).map_err(database_error)?,
        row.get(5).map_err(database_error)?,
    ))
}

/// Wraps a Turso failure in the authentication error surface.
fn database_error(source: turso::Error) -> AuthError {
    AuthError::Database(Box::new(source))
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
