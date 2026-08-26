use thiserror::Error;

/// A typed authentication initialization or backend failure.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("username must be 1 to 64 lowercase letters, numbers, hyphens, or underscores")]
    InvalidUsername,
    #[error("password must contain between 12 and 128 characters")]
    InvalidPassword,
    #[error("at least one valid lowercase snake-case role is required")]
    InvalidRoles,
    #[error("an account with that username already exists")]
    DuplicateUser,
    #[error("authentication database operation failed")]
    Database(#[source] Box<turso::Error>),
    #[error("authentication database row could not be decoded: {0}")]
    DatabaseDecode(String),
    #[error("password hashing failed")]
    PasswordHash,
    #[error("password worker failed")]
    PasswordWorker(#[source] tokio::task::JoinError),
    #[error("authentication tables are unavailable; apply the auth migration")]
    MissingSchema,
}
