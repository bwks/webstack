use std::{fs, path::PathBuf};

use surrealdb::types::SurrealValue;
use tempfile::TempDir;
use webstack_db::{Database, DatabaseError, connect, migrate};

#[derive(Debug, SurrealValue)]
struct PersistedValue {
    payload: String,
}

#[derive(Debug, SurrealValue, PartialEq, Eq)]
struct Sequence {
    sequence: u8,
}

#[derive(Debug, SurrealValue)]
struct Count {
    count: usize,
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

async fn reopen(path: &std::path::Path, namespace: &str, database: &str) -> Database {
    for _attempt in 0..100 {
        match connect(path, namespace, database).await {
            Ok(database) => return database,
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
        }
    }
    panic!("database lock was not released after all handles were dropped");
}

#[tokio::test]
async fn rocksdb_connection_selects_scope_shares_and_reopens() {
    let directory = TempDir::new().expect("temporary directory");
    let database = connect(directory.path(), "inventory", "production")
        .await
        .expect("database connection");
    let clone = database.clone();
    database
        .query("CREATE persistence:one SET payload = 'kept';")
        .await
        .expect("create query")
        .check()
        .expect("create statement");
    let mut response = clone
        .query("SELECT payload FROM persistence:one;")
        .await
        .expect("select query")
        .check()
        .expect("select statement");
    let values: Vec<PersistedValue> = response.take(0).expect("values");
    assert_eq!(values[0].payload, "kept");
    drop(response);
    drop(clone);
    drop(database);

    let reopened = reopen(directory.path(), "inventory", "production").await;
    let mut response = reopened
        .query("SELECT payload FROM persistence:one;")
        .await
        .expect("reopen query")
        .check()
        .expect("reopen statement");
    let values: Vec<PersistedValue> = response.take(0).expect("values");
    assert_eq!(values[0].payload, "kept");
}

#[tokio::test]
async fn migrations_apply_in_order_and_are_restart_idempotent() {
    let directory = TempDir::new().expect("temporary directory");
    let database = connect(directory.path(), "test", "ordered")
        .await
        .expect("database");
    migrate(&database, &fixture("ordered"))
        .await
        .expect("first migration run");
    migrate(&database, &fixture("ordered"))
        .await
        .expect("idempotent migration run");

    let mut response = database
        .query("SELECT sequence FROM migration_event ORDER BY sequence;")
        .await
        .expect("events")
        .check()
        .expect("event query");
    let sequences: Vec<Sequence> = response.take(0).expect("sequences");
    assert_eq!(
        sequences,
        vec![Sequence { sequence: 1 }, Sequence { sequence: 2 }]
    );

    let mut response = database
        .query("SELECT count() AS count FROM _migrations GROUP ALL;")
        .await
        .expect("ledger")
        .check()
        .expect("ledger query");
    let counts: Vec<Count> = response.take(0).expect("count");
    assert_eq!(counts[0].count, 2);
}

#[tokio::test]
async fn failed_migration_rolls_back_statements_and_ledger() {
    let directory = TempDir::new().expect("temporary directory");
    let database = connect(directory.path(), "test", "rollback")
        .await
        .expect("database");
    assert!(matches!(
        migrate(&database, &fixture("failing")).await,
        Err(DatabaseError::Migration { .. })
    ));

    let response = database
        .query("SELECT * FROM rollback_event;")
        .await
        .expect("rollback query");
    assert!(
        response.check().is_err(),
        "rolled-back table must not exist"
    );
    let mut response = database
        .query("SELECT * FROM _migrations;")
        .await
        .expect("ledger query")
        .check()
        .expect("ledger statement");
    let rows: Vec<surrealdb::types::Value> = response.take(0).expect("rows");
    assert!(rows.is_empty());
}

#[tokio::test]
async fn startup_rejects_checksum_drift_and_missing_applied_files() {
    let drift_directory = TempDir::new().expect("temporary directory");
    let database = connect(drift_directory.path(), "test", "drift")
        .await
        .expect("database");
    migrate(&database, &fixture("original"))
        .await
        .expect("original migration");
    assert!(matches!(
        migrate(&database, &fixture("drifted")).await,
        Err(DatabaseError::ChecksumDrift { .. })
    ));

    let missing_directory = TempDir::new().expect("temporary directory");
    let database = connect(missing_directory.path(), "test", "missing")
        .await
        .expect("database");
    migrate(&database, &fixture("original"))
        .await
        .expect("original migration");
    fs::create_dir(missing_directory.path().join("empty")).expect("empty migration directory");
    assert!(matches!(
        migrate(&database, &missing_directory.path().join("empty")).await,
        Err(DatabaseError::MissingAppliedMigration(_))
    ));
}

#[tokio::test]
async fn invalid_filesystem_histories_fail_before_application() {
    let invalid_directory = TempDir::new().expect("temporary directory");
    let database = connect(invalid_directory.path(), "test", "invalid")
        .await
        .expect("database");
    assert!(matches!(
        migrate(&database, &fixture("invalid")).await,
        Err(DatabaseError::InvalidMigrationPath(_))
    ));

    let duplicate_directory = TempDir::new().expect("temporary directory");
    let database = connect(duplicate_directory.path(), "test", "duplicate")
        .await
        .expect("database");
    assert!(matches!(
        migrate(&database, &fixture("duplicate")).await,
        Err(DatabaseError::DuplicateMigrationPrefix { .. })
    ));
}

#[tokio::test]
async fn missing_directory_fails_and_an_empty_directory_is_valid() {
    let directory = TempDir::new().expect("temporary directory");
    let database = connect(&directory.path().join("database"), "test", "directory")
        .await
        .expect("database");
    let migrations = directory.path().join("migrations");
    assert!(matches!(
        migrate(&database, &migrations).await,
        Err(DatabaseError::ReadMigrationDirectory { .. })
    ));

    let not_a_directory = directory.path().join("migrations-file");
    fs::write(&not_a_directory, "not a directory").expect("non-directory migration path");
    assert!(matches!(
        migrate(&database, &not_a_directory).await,
        Err(DatabaseError::ReadMigrationDirectory { .. })
    ));

    fs::create_dir(&migrations).expect("empty migration directory");
    migrate(&database, &migrations)
        .await
        .expect("empty migration history");
}

#[tokio::test]
async fn nested_directories_and_non_utf8_contents_are_rejected() {
    let directory = TempDir::new().expect("temporary directory");
    let database = connect(&directory.path().join("database"), "test", "paths")
        .await
        .expect("database");
    let migrations = directory.path().join("migrations");
    fs::create_dir(&migrations).expect("migration directory");
    fs::create_dir(migrations.join("nested")).expect("nested directory");
    assert!(matches!(
        migrate(&database, &migrations).await,
        Err(DatabaseError::InvalidMigrationPath(_))
    ));

    fs::remove_dir(migrations.join("nested")).expect("remove nested directory");
    fs::write(migrations.join("0001_invalid_utf8.surql"), [0xff]).expect("invalid UTF-8 migration");
    assert!(matches!(
        migrate(&database, &migrations).await,
        Err(DatabaseError::NonUtf8Migration(_))
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn migration_symlinks_are_rejected() {
    use std::os::unix::fs::symlink;

    let directory = TempDir::new().expect("temporary directory");
    let database = connect(&directory.path().join("database"), "test", "symlink")
        .await
        .expect("database");
    let migrations = directory.path().join("migrations");
    fs::create_dir(&migrations).expect("migration directory");
    let target = directory.path().join("target.surql");
    fs::write(&target, "RETURN true;").expect("symlink target");
    symlink(&target, migrations.join("0001_link.surql")).expect("migration symlink");

    assert!(matches!(
        migrate(&database, &migrations).await,
        Err(DatabaseError::InvalidMigrationPath(_))
    ));
}
