use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
};

use flate2::read::GzDecoder;
use webstack_core::config::Config;

use crate::{
    BackupError,
    backup::remove_database_files,
    store::{ObjectStore, R2Store},
};

/// Environment variable naming a local backup file or R2 object key to restore.
pub const RESTORE_ENV: &str = "APP_RESTORE_FROM";

const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Restores `APP_RESTORE_FROM` into the configured database path when set.
///
/// The value is treated as a local file when one exists at that path and as
/// an R2 object key otherwise. Restoring requires that no database exists yet.
/// Returns the restored source, or `None` when no restore was requested.
///
/// # Errors
///
/// Returns an error when the database already exists, the source cannot be
/// read, or the restored file fails its integrity check.
pub async fn restore_if_requested(config: &Config) -> Result<Option<String>, BackupError> {
    let Some(source) = env::var_os(RESTORE_ENV) else {
        return Ok(None);
    };
    let source = source
        .into_string()
        .map_err(|_| BackupError::NonUnicodeRestoreSource)?;
    let store = if Path::new(&source).is_file() {
        None
    } else {
        let r2 = &config.backup.r2;
        if r2.account_id.trim().is_empty() || r2.bucket.trim().is_empty() {
            return Err(BackupError::RestoreNotConfigured);
        }
        Some(R2Store::from_config(r2)?)
    };
    restore(&source, &config.database.path, store.as_ref()).await?;
    Ok(Some(source))
}

/// Restores a local file, or an object from `store`, into a fresh database path.
pub(crate) async fn restore(
    source: &str,
    database_path: &Path,
    store: Option<&impl ObjectStore>,
) -> Result<(), BackupError> {
    if database_path.exists() || sidecar(database_path, "-wal").exists() {
        return Err(BackupError::RestoreTargetExists(database_path.to_owned()));
    }
    let bytes = match store {
        None => fs::read(source).map_err(|error| BackupError::Io {
            path: PathBuf::from(source),
            source: error,
        })?,
        Some(store) => store.get(source).await?,
    };
    let bytes = if bytes.starts_with(&GZIP_MAGIC) {
        let mut decompressed = Vec::new();
        GzDecoder::new(bytes.as_slice())
            .read_to_end(&mut decompressed)
            .map_err(|error| BackupError::Io {
                path: PathBuf::from(source),
                source: error,
            })?;
        decompressed
    } else {
        bytes
    };

    let staged = sidecar(database_path, ".restore");
    remove_database_files(&staged);
    if let Some(parent) = staged
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| BackupError::Io {
            path: parent.to_owned(),
            source: error,
        })?;
    }
    fs::write(&staged, bytes).map_err(|error| BackupError::Io {
        path: staged.clone(),
        source: error,
    })?;
    let checked = integrity_check(&staged).await;
    if let Err(error) = checked {
        remove_database_files(&staged);
        return Err(error);
    }
    for suffix in ["-wal", "-shm"] {
        let _ignored = fs::remove_file(sidecar(&staged, suffix));
    }
    fs::rename(&staged, database_path).map_err(|error| BackupError::Io {
        path: database_path.to_owned(),
        source: error,
    })?;
    tracing::warn!(source = %source, path = %database_path.display(), "database restored from backup");
    Ok(())
}

/// Opens a staged database and requires `PRAGMA integrity_check` to report ok.
async fn integrity_check(path: &Path) -> Result<(), BackupError> {
    let database = turso::Builder::new_local(&path.to_string_lossy())
        .build()
        .await
        .map_err(|error| BackupError::Database(Box::new(error)))?;
    let connection = database
        .connect()
        .map_err(|error| BackupError::Database(Box::new(error)))?;
    let mut rows = connection
        .query("PRAGMA integrity_check", ())
        .await
        .map_err(|error| BackupError::Database(Box::new(error)))?;
    let result: String = match rows.next().await {
        Ok(Some(row)) => row
            .get(0)
            .map_err(|error| BackupError::Database(Box::new(error)))?,
        Ok(None) => String::new(),
        Err(error) => return Err(BackupError::Database(Box::new(error))),
    };
    if result == "ok" {
        Ok(())
    } else {
        Err(BackupError::Integrity(result))
    }
}

/// Appends a suffix to a path's final component.
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};
    use tempfile::tempdir;

    use super::restore;
    use crate::{BackupError, store::tests::MemoryStore};

    async fn sample_database(path: &std::path::Path) -> Vec<u8> {
        let database = webstack_db::connect(path).await.expect("database");
        let connection = database.connection().await.expect("connection");
        connection
            .execute("CREATE TABLE note (body TEXT)", ())
            .await
            .expect("table");
        connection
            .execute("INSERT INTO note VALUES ('restored')", ())
            .await
            .expect("row");
        let snapshot = path.with_file_name("snapshot.db");
        connection
            .execute(&format!("VACUUM INTO '{}'", snapshot.display()), ())
            .await
            .expect("snapshot");
        std::fs::read(snapshot).expect("snapshot bytes")
    }

    async fn note(path: &std::path::Path) -> String {
        let database = webstack_db::connect(path).await.expect("restored database");
        let connection = database.connection().await.expect("connection");
        let mut rows = connection
            .query("SELECT body FROM note", ())
            .await
            .expect("query");
        rows.next()
            .await
            .expect("row")
            .expect("value")
            .get(0)
            .expect("body")
    }

    #[tokio::test]
    async fn gzipped_r2_backups_restore_into_a_fresh_database_path() {
        let data = tempdir().expect("data directory");
        let snapshot = sample_database(&data.path().join("source.db")).await;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&snapshot).expect("gzip");
        let store = MemoryStore::default();
        store.objects.lock().expect("objects lock").insert(
            "db/20261024T030000Z.db.gz".to_owned(),
            encoder.finish().expect("gzip bytes"),
        );
        let target = data.path().join("fresh/app.db");

        restore("db/20261024T030000Z.db.gz", &target, Some(&store))
            .await
            .expect("restored backup");

        assert_eq!(note(&target).await, "restored");
        assert!(!data.path().join("fresh/app.db.restore").exists());
    }

    #[tokio::test]
    async fn plain_local_snapshots_restore_and_existing_databases_are_protected() {
        let data = tempdir().expect("data directory");
        let snapshot = sample_database(&data.path().join("source.db")).await;
        let local = data.path().join("backup.db");
        std::fs::write(&local, snapshot).expect("local backup");
        let target = data.path().join("app.db");

        restore(local.to_str().expect("path"), &target, None::<&MemoryStore>)
            .await
            .expect("restored local backup");
        assert_eq!(note(&target).await, "restored");

        let error = restore(local.to_str().expect("path"), &target, None::<&MemoryStore>)
            .await
            .expect_err("existing database");
        assert!(matches!(error, BackupError::RestoreTargetExists(_)));
    }

    #[tokio::test]
    async fn corrupt_backups_are_rejected_without_creating_a_database() {
        let data = tempdir().expect("data directory");
        let local = data.path().join("corrupt.db");
        std::fs::write(&local, b"not a database at all").expect("corrupt backup");
        let target = data.path().join("app.db");

        let error = restore(local.to_str().expect("path"), &target, None::<&MemoryStore>)
            .await
            .expect_err("corrupt backup");

        assert!(matches!(
            error,
            BackupError::Database(_) | BackupError::Integrity(_)
        ));
        assert!(!target.exists());
    }
}
