use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use surrealdb::types::SurrealValue;

use crate::{connection::Database, error::DatabaseError, validation::filesystem_migrations};

const LEDGER_SETUP: &str = r"
DEFINE TABLE IF NOT EXISTS _migrations SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS filename ON _migrations TYPE string;
DEFINE FIELD IF NOT EXISTS checksum ON _migrations TYPE string;
DEFINE FIELD IF NOT EXISTS applied_at ON _migrations TYPE datetime;
DEFINE INDEX IF NOT EXISTS migration_filename ON _migrations FIELDS filename UNIQUE;
";
const LEDGER_SELECT: &str = "SELECT filename, checksum FROM _migrations ORDER BY filename;";
const LEDGER_INSERT: &str =
    "CREATE _migrations SET filename = $filename, checksum = $checksum, applied_at = time::now();";

/// One validated filesystem migration ready for startup application.
pub(crate) struct Migration {
    pub(crate) filename: String,
    pub(crate) source: String,
    pub(crate) checksum: String,
}

/// One row from Webstack's migration ledger.
#[derive(SurrealValue)]
struct AppliedMigration {
    filename: String,
    checksum: String,
}

/// Validates and applies an application's runtime migrations in filename order.
///
/// Each migration and its ledger record commit in one client transaction.
///
/// # Errors
///
/// Returns an error for an unreadable directory, invalid input, ledger drift, or any database
/// failure while applying or recording migrations.
pub async fn migrate(database: &Database, directory: &Path) -> Result<(), DatabaseError> {
    let migrations = filesystem_migrations(directory)?;
    database
        .query(LEDGER_SETUP)
        .await
        .and_then(surrealdb::IndexedResults::check)
        .map_err(|source| DatabaseError::Ledger {
            operation: "initialize",
            source: Box::new(source),
        })?;

    let mut response = database
        .query(LEDGER_SELECT)
        .await
        .and_then(surrealdb::IndexedResults::check)
        .map_err(|source| DatabaseError::Ledger {
            operation: "read",
            source: Box::new(source),
        })?;
    let applied: Vec<AppliedMigration> =
        response.take(0).map_err(|source| DatabaseError::Ledger {
            operation: "decode",
            source: Box::new(source),
        })?;
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
    let transaction =
        database
            .clone()
            .begin()
            .await
            .map_err(|source| DatabaseError::Migration {
                filename: migration.filename.clone(),
                source: Box::new(source),
            })?;

    if let Err(source) = transaction
        .query(&migration.source)
        .await
        .and_then(surrealdb::IndexedResults::check)
    {
        let _cancel_result = transaction.cancel().await;
        return Err(DatabaseError::Migration {
            filename: migration.filename.clone(),
            source: Box::new(source),
        });
    }
    if let Err(source) = transaction
        .query(LEDGER_INSERT)
        .bind(("filename", migration.filename.clone()))
        .bind(("checksum", migration.checksum.clone()))
        .await
        .and_then(surrealdb::IndexedResults::check)
    {
        let _cancel_result = transaction.cancel().await;
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
