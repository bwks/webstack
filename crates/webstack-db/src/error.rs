use std::{io, path::PathBuf};

use thiserror::Error;

/// A typed embedded database or migration failure.
#[derive(Debug, Error)]
pub enum DatabaseError {
    /// The configured database parent directory could not be created.
    #[error("cannot create database directory {}", path.display())]
    CreateParent {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// Turso could not open the configured database file.
    #[error("cannot open embedded database at {}", path.display())]
    Connect {
        path: PathBuf,
        #[source]
        source: Box<turso::Error>,
    },
    /// The required database pragmas could not be configured.
    #[error("cannot configure embedded database at {}", path.display())]
    Configure {
        path: PathBuf,
        #[source]
        source: Box<turso::Error>,
    },
    /// A framework-owned migration ledger query failed.
    #[error("cannot {operation} the migration ledger")]
    Ledger {
        operation: &'static str,
        #[source]
        source: Box<turso::Error>,
    },
    /// The runtime migration directory could not be read.
    #[error("cannot read migration directory {}", path.display())]
    ReadMigrationDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// A runtime migration file or its metadata could not be read.
    #[error("cannot read migration path {}", path.display())]
    ReadMigrationPath {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// A path is not a flat, regular migration file with a valid name.
    #[error("invalid migration path {}; expected a regular NNNN_lowercase_name.sql file", .0.display())]
    InvalidMigrationPath(PathBuf),
    /// Two runtime migrations use the same numeric prefix.
    #[error("migrations {first:?} and {second:?} use the same numeric prefix")]
    DuplicateMigrationPrefix { first: String, second: String },
    /// A runtime migration is not valid UTF-8.
    #[error("migration {} is not valid UTF-8", .0.display())]
    NonUtf8Migration(PathBuf),
    /// Migration files may not manage their own transaction.
    #[error("migration {filename:?} contains forbidden transaction keyword {keyword}")]
    TransactionControl {
        filename: String,
        keyword: &'static str,
    },
    /// A ledger entry no longer has a corresponding runtime migration.
    #[error("applied migration {0:?} is missing from the runtime migration directory")]
    MissingAppliedMigration(String),
    /// An applied migration changed after it was recorded.
    #[error("applied migration {filename:?} checksum changed")]
    ChecksumDrift { filename: String },
    /// A migration statement batch failed and was rolled back.
    #[error("migration {filename:?} failed")]
    Migration {
        filename: String,
        #[source]
        source: Box<turso::Error>,
    },
    /// A migration ledger insert failed and was rolled back.
    #[error("cannot record migration {filename:?}")]
    RecordMigration {
        filename: String,
        #[source]
        source: Box<turso::Error>,
    },
    /// Committing a migration and its ledger row failed.
    #[error("cannot commit migration {filename:?}")]
    CommitMigration {
        filename: String,
        #[source]
        source: Box<turso::Error>,
    },
}
