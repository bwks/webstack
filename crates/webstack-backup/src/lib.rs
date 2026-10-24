#![doc = "Consistent database snapshot backup to Cloudflare R2, retention, scheduling, and restore."]

mod backup;
mod restore;
mod schedule;
mod store;

use std::{io, path::PathBuf};

use thiserror::Error;
use tokio::task::JoinError;

pub use backup::Backups;
pub use restore::{RESTORE_ENV, restore_if_requested};
pub use schedule::spawn_schedule;

/// A typed backup, restore, or object storage failure.
#[derive(Debug, Error)]
pub enum BackupError {
    #[error("R2 credentials are required; set R2_ACCESS_KEY_ID and R2_SECRET_ACCESS_KEY")]
    MissingCredentials,
    #[error("restoring from R2 requires backup.r2.account_id and backup.r2.bucket")]
    RestoreNotConfigured,
    #[error("APP_RESTORE_FROM is not valid Unicode")]
    NonUnicodeRestoreSource,
    #[error(
        "cannot restore over existing database {}; move it aside or unset APP_RESTORE_FROM",
        .0.display()
    )]
    RestoreTargetExists(PathBuf),
    #[error("restored database failed its integrity check: {0}")]
    Integrity(String),
    #[error("database snapshot failed")]
    Database(#[source] Box<turso::Error>),
    #[error("backup file operation failed at {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("R2 request failed: {0}")]
    Storage(String),
    #[error("invalid backup schedule: {0}")]
    Schedule(String),
    #[error("backup task failed")]
    Task(#[source] JoinError),
}
