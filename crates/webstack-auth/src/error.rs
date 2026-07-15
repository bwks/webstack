use thiserror::Error;

/// A typed authentication initialization or backend failure.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("authentication database operation failed")]
    Database(#[source] Box<surrealdb::Error>),
    #[error("password hashing failed")]
    PasswordHash,
    #[error("password worker failed")]
    PasswordWorker(#[source] tokio::task::JoinError),
    #[error("authentication tables are unavailable; apply the auth migration")]
    MissingSchema,
}
