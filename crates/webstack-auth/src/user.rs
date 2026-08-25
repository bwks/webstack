use axum_login::AuthUser;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// One local Webstack account.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct User {
    pub(crate) username: String,
    #[serde(skip_serializing)]
    pub(crate) password_hash: String,
    pub(crate) roles: Vec<String>,
    pub(crate) disabled: bool,
    pub(crate) created_at: i64,
    pub(crate) password_expires_at: i64,
}

impl User {
    /// Builds a user from validated database fields.
    pub(crate) fn from_database(
        username: String,
        password_hash: String,
        roles: Vec<String>,
        disabled: bool,
        created_at: i64,
        password_expires_at: i64,
    ) -> Self {
        Self {
            username,
            password_hash,
            roles,
            disabled,
            created_at,
            password_expires_at,
        }
    }

    /// Returns the normalized account username.
    #[must_use]
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Returns the account's assigned roles.
    #[must_use]
    pub fn roles(&self) -> &[String] {
        &self.roles
    }

    /// Reports whether this account is disabled.
    #[must_use]
    pub const fn disabled(&self) -> bool {
        self.disabled
    }

    /// Returns the required password replacement time.
    #[must_use]
    pub fn password_expires_at(&self) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(self.password_expires_at)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH)
    }

    /// Reports whether the password must be replaced now.
    #[must_use]
    pub fn password_expired(&self) -> bool {
        self.password_expires_at() <= OffsetDateTime::now_utc()
    }

    /// Reports whether the user has an exact role.
    #[must_use]
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|candidate| candidate == role)
    }
}

impl AuthUser for User {
    type Id = String;

    /// Returns the stable normalized username used as the account ID.
    fn id(&self) -> Self::Id {
        self.username.clone()
    }

    /// Returns the password hash used to invalidate stale sessions.
    fn session_auth_hash(&self) -> &[u8] {
        self.password_hash.as_bytes()
    }
}
