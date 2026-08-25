use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use time::OffsetDateTime;
use turso::transaction::TransactionBehavior;

use crate::{connection::Database, error::DatabaseError, validation::filesystem_migrations};

const LEDGER_SETUP: &str = r"
CREATE TABLE IF NOT EXISTS _migrations (
    filename TEXT PRIMARY KEY,
    checksum TEXT NOT NULL,
    applied_at INTEGER NOT NULL
) STRICT;
";
const LEDGER_SELECT: &str = "SELECT filename, checksum FROM _migrations ORDER BY filename";
const LEDGER_INSERT: &str =
    "INSERT INTO _migrations (filename, checksum, applied_at) VALUES (?1, ?2, ?3)";

/// One validated filesystem migration ready for startup application.
pub(crate) struct Migration {
    pub(crate) filename: String,
    pub(crate) source: String,
    pub(crate) checksum: String,
}

/// One row from Webstack's migration ledger.
struct AppliedMigration {
    filename: String,
    checksum: String,
}

/// Validates and applies an application's runtime migrations in filename order.
///
/// Each migration and its ledger record commit in one database transaction.
///
/// # Errors
///
/// Returns an error for an unreadable directory, invalid input, ledger drift, or any database
/// failure while applying or recording migrations.
pub async fn migrate(database: &Database, directory: &Path) -> Result<(), DatabaseError> {
    let migrations = filesystem_migrations(directory)?;
    let connection = database
        .connection()
        .await
        .map_err(|source| DatabaseError::Ledger {
            operation: "connect",
            source: Box::new(source),
        })?;
    connection
        .execute_batch(LEDGER_SETUP)
        .await
        .map_err(|source| DatabaseError::Ledger {
            operation: "initialize",
            source: Box::new(source),
        })?;

    let mut rows = connection
        .query(LEDGER_SELECT, ())
        .await
        .map_err(|source| DatabaseError::Ledger {
            operation: "read",
            source: Box::new(source),
        })?;
    let mut applied = Vec::new();
    while let Some(row) = rows.next().await.map_err(|source| DatabaseError::Ledger {
        operation: "read",
        source: Box::new(source),
    })? {
        applied.push(AppliedMigration {
            filename: row.get(0).map_err(|source| DatabaseError::Ledger {
                operation: "decode",
                source: Box::new(source),
            })?,
            checksum: row.get(1).map_err(|source| DatabaseError::Ledger {
                operation: "decode",
                source: Box::new(source),
            })?,
        });
    }
    verify_ledger(&migrations, &applied)?;

    let applied_names: BTreeSet<&str> = applied
        .iter()
        .map(|migration| migration.filename.as_str())
        .collect();
    for migration in migrations
        .iter()
        .filter(|migration| !applied_names.contains(migration.filename.as_str()))
    {
        apply_migration(database, migration).await?;
    }
    Ok(())
}

/// Applies one migration and records it atomically.
async fn apply_migration(database: &Database, migration: &Migration) -> Result<(), DatabaseError> {
    let mut connection =
        database
            .connection()
            .await
            .map_err(|source| DatabaseError::Migration {
                filename: migration.filename.clone(),
                source: Box::new(source),
            })?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await
        .map_err(|source| DatabaseError::Migration {
            filename: migration.filename.clone(),
            source: Box::new(source),
        })?;

    if let Err(source) = transaction.execute_batch(&migration.source).await {
        let _rollback_result = transaction.rollback().await;
        return Err(DatabaseError::Migration {
            filename: migration.filename.clone(),
            source: Box::new(source),
        });
    }
    if let Err(source) = transaction
        .execute(
            LEDGER_INSERT,
            (
                migration.filename.as_str(),
                migration.checksum.as_str(),
                OffsetDateTime::now_utc().unix_timestamp(),
            ),
        )
        .await
    {
        let _rollback_result = transaction.rollback().await;
        return Err(DatabaseError::RecordMigration {
            filename: migration.filename.clone(),
            source: Box::new(source),
        });
    }
    transaction
        .commit()
        .await
        .map_err(|source| DatabaseError::CommitMigration {
            filename: migration.filename.clone(),
            source: Box::new(source),
        })?;
    Ok(())
}

/// Compares the available runtime migration history with the durable ledger.
fn verify_ledger(
    migrations: &[Migration],
    applied: &[AppliedMigration],
) -> Result<(), DatabaseError> {
    let available = migrations
        .iter()
        .map(|migration| (migration.filename.as_str(), migration.checksum.as_str()))
        .collect::<BTreeMap<_, _>>();
    for migration in applied {
        let Some(checksum) = available.get(migration.filename.as_str()) else {
            return Err(DatabaseError::MissingAppliedMigration(
                migration.filename.clone(),
            ));
        };
        if *checksum != migration.checksum {
            return Err(DatabaseError::ChecksumDrift {
                filename: migration.filename.clone(),
            });
        }
    }
    Ok(())
}
