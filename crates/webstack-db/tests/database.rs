use std::{fs, path::PathBuf};

use tempfile::TempDir;
use turso::transaction::TransactionBehavior;
use webstack_db::{DatabaseError, connect, migrate};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[tokio::test]
async fn turso_connection_shares_and_reopens_persistent_data() {
    let directory = TempDir::new().expect("temporary directory");
    let path = directory.path().join("nested/app.db");
    let database = connect(&path).await.expect("database connection");
    let clone = database.clone();
    let connection = database.connection().await.expect("connection");
    connection
        .execute_batch(
            "CREATE TABLE persistence (id INTEGER PRIMARY KEY, payload TEXT NOT NULL) STRICT; \
             INSERT INTO persistence (id, payload) VALUES (1, 'kept');",
        )
        .await
        .expect("create data");
    let connection = clone.connection().await.expect("clone connection");
    let mut rows = connection
        .query("SELECT payload FROM persistence WHERE id = 1", ())
        .await
        .expect("select query");
    assert_eq!(
        rows.next()
            .await
            .expect("row")
            .expect("value")
            .get::<String>(0)
            .expect("payload"),
        "kept"
    );
    drop(rows);
    drop(connection);
    drop(clone);
    drop(database);

    let reopened = connect(&path).await.expect("reopen database");
    let connection = reopened.connection().await.expect("reopen connection");
    let mut rows = connection
        .query("SELECT payload FROM persistence WHERE id = 1", ())
        .await
        .expect("reopen query");
    assert_eq!(
        rows.next()
            .await
            .expect("row")
            .expect("value")
            .get::<String>(0)
            .expect("payload"),
        "kept"
    );
}

#[tokio::test]
async fn migrations_apply_in_order_and_are_restart_idempotent() {
    let directory = TempDir::new().expect("temporary directory");
    let database = connect(&directory.path().join("app.db"))
        .await
        .expect("database");
    migrate(&database, &fixture("ordered"))
        .await
        .expect("first migration run");
    migrate(&database, &fixture("ordered"))
        .await
        .expect("idempotent migration run");

    let connection = database.connection().await.expect("connection");
    let mut rows = connection
        .query("SELECT sequence FROM migration_event ORDER BY sequence", ())
        .await
        .expect("events");
    let mut sequences = Vec::new();
    while let Some(row) = rows.next().await.expect("event row") {
        sequences.push(row.get::<i64>(0).expect("sequence"));
    }
    assert_eq!(sequences, vec![1, 2]);
    let mut rows = connection
        .query("SELECT COUNT(*) FROM _migrations", ())
        .await
        .expect("ledger");
    assert_eq!(
        rows.next()
            .await
            .expect("count row")
            .expect("count")
            .get::<i64>(0)
            .expect("count value"),
        2
    );
}

#[tokio::test]
async fn demo_migrations_create_a_working_items_schema() {
    let directory = TempDir::new().expect("temporary directory");
    let database = connect(&directory.path().join("app.db"))
        .await
        .expect("database");
    let migrations =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/migrations");
    migrate(&database, &migrations)
        .await
        .expect("demo migrations");

    let connection = database.connection().await.expect("connection");
    connection
        .execute(
            "INSERT INTO item (id, name) VALUES (?1, ?2)",
            ("item-one", "First"),
        )
        .await
        .expect("create item");
    assert_eq!(
        connection
            .execute(
                "UPDATE item SET name = ?1, updated_at = unixepoch() WHERE id = ?2",
                ("Updated", "item-one"),
            )
            .await
            .expect("update item"),
        1
    );
    let mut rows = connection
        .query("SELECT name FROM item WHERE id = ?1", ("item-one",))
        .await
        .expect("select item");
    assert_eq!(
        rows.next()
            .await
            .expect("item row")
            .expect("item")
            .get::<String>(0)
            .expect("name"),
        "Updated"
    );
    drop(rows);
    assert_eq!(
        connection
            .execute("DELETE FROM item WHERE id = ?1", ("item-one",))
            .await
            .expect("delete item"),
        1
    );
}

#[tokio::test]
async fn failed_migration_rolls_back_statements_and_ledger() {
    let directory = TempDir::new().expect("temporary directory");
    let database = connect(&directory.path().join("app.db"))
        .await
        .expect("database");
    assert!(matches!(
        migrate(&database, &fixture("failing")).await,
        Err(DatabaseError::Migration { .. })
    ));

    let connection = database.connection().await.expect("connection");
    assert!(
        connection
            .query("SELECT * FROM rollback_event", ())
            .await
            .is_err(),
        "rolled-back table must not exist"
    );
    let mut rows = connection
        .query("SELECT COUNT(*) FROM _migrations", ())
        .await
        .expect("ledger query");
    assert_eq!(
        rows.next()
            .await
            .expect("ledger row")
            .expect("ledger count")
            .get::<i64>(0)
            .expect("count"),
        0
    );
}

#[tokio::test]
async fn startup_rejects_checksum_drift_and_missing_applied_files() {
    let drift_directory = TempDir::new().expect("temporary directory");
    let database = connect(&drift_directory.path().join("app.db"))
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
    let database = connect(&missing_directory.path().join("app.db"))
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
    let database = connect(&invalid_directory.path().join("app.db"))
        .await
        .expect("database");
    assert!(matches!(
        migrate(&database, &fixture("invalid")).await,
        Err(DatabaseError::InvalidMigrationPath(_))
    ));

    let duplicate_directory = TempDir::new().expect("temporary directory");
    let database = connect(&duplicate_directory.path().join("app.db"))
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
    let database = connect(&directory.path().join("app.db"))
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
    let database = connect(&directory.path().join("app.db"))
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
    fs::write(migrations.join("0001_invalid_utf8.sql"), [0xff]).expect("invalid UTF-8 migration");
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
    let database = connect(&directory.path().join("app.db"))
        .await
        .expect("database");
    let migrations = directory.path().join("migrations");
    fs::create_dir(&migrations).expect("migration directory");
    let target = directory.path().join("target.sql");
    fs::write(&target, "SELECT 1;").expect("symlink target");
    symlink(&target, migrations.join("0001_link.sql")).expect("migration symlink");

    assert!(matches!(
        migrate(&database, &migrations).await,
        Err(DatabaseError::InvalidMigrationPath(_))
    ));
}

#[tokio::test]
async fn wal_allows_readers_while_one_writer_transaction_is_active() {
    let directory = TempDir::new().expect("temporary directory");
    let database = connect(&directory.path().join("app.db"))
        .await
        .expect("database");
    let mut writer = database.connection().await.expect("writer connection");
    writer
        .execute_batch(
            "CREATE TABLE concurrency (id INTEGER PRIMARY KEY) STRICT; \
             INSERT INTO concurrency (id) VALUES (1);",
        )
        .await
        .expect("initial data");

    let transaction = writer
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await
        .expect("writer transaction");
    transaction
        .execute("INSERT INTO concurrency (id) VALUES (2)", ())
        .await
        .expect("uncommitted write");

    let reader = database.connection().await.expect("reader connection");
    let mut rows = reader
        .query("SELECT COUNT(*) FROM concurrency", ())
        .await
        .expect("read during write transaction");
    assert_eq!(
        rows.next()
            .await
            .expect("count row")
            .expect("count")
            .get::<i64>(0)
            .expect("count value"),
        1,
        "reader sees the last committed snapshot"
    );
    drop(rows);

    transaction.commit().await.expect("commit writer");
    let mut rows = reader
        .query("SELECT COUNT(*) FROM concurrency", ())
        .await
        .expect("read committed data");
    assert_eq!(
        rows.next()
            .await
            .expect("count row")
            .expect("count")
            .get::<i64>(0)
            .expect("count value"),
        2
    );
}
