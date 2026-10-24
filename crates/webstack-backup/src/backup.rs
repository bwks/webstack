use std::{
    fs, io,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use flate2::{Compression, write::GzEncoder};
use tokio::sync::Mutex;
use uuid::Uuid;
use webstack_core::config::Config;
use webstack_db::Database;

use crate::{
    BackupError,
    store::{ObjectStore, R2Store},
};

const KEY_SUFFIX: &str = ".db.gz";
const KEY_TIMESTAMP: &str = "%Y%m%dT%H%M%SZ";
const KEY_TIMESTAMP_LENGTH: usize = 16;
const WORK_DIRECTORY: &str = ".webstack-backup";

/// Where snapshots are staged and how many uploaded backups are kept.
pub(crate) struct BackupSettings {
    pub(crate) prefix: String,
    pub(crate) retention: usize,
    pub(crate) work_dir: PathBuf,
}

impl BackupSettings {
    /// Derives backup settings from validated configuration.
    fn from_config(config: &Config) -> Self {
        let work_dir = config
            .database
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .join(WORK_DIRECTORY);
        Self {
            prefix: config.backup.r2.prefix.clone(),
            retention: config.backup.retention,
            work_dir,
        }
    }
}

/// Runs consistent database backups to R2, one at a time.
pub struct Backups {
    database: Database,
    store: R2Store,
    settings: BackupSettings,
    running: Mutex<()>,
}

impl Backups {
    /// Prepares R2 backups for the configured database.
    ///
    /// # Errors
    ///
    /// Returns [`BackupError::MissingCredentials`] when R2 credentials are absent.
    pub fn from_config(config: &Config, database: Database) -> Result<Self, BackupError> {
        Ok(Self {
            database,
            store: R2Store::from_config(&config.backup.r2)?,
            settings: BackupSettings::from_config(config),
            running: Mutex::new(()),
        })
    }

    /// Takes one backup now, waiting for any backup already in progress.
    ///
    /// Returns the uploaded object key.
    ///
    /// # Errors
    ///
    /// Returns a snapshot, compression, or upload failure. Retention pruning
    /// failures are logged without failing the completed backup.
    pub async fn run(&self) -> Result<String, BackupError> {
        let _running = self.running.lock().await;
        back_up(&self.database, &self.store, &self.settings, Utc::now()).await
    }
}

/// Snapshots, compresses, uploads, and prunes one backup.
pub(crate) async fn back_up(
    database: &Database,
    store: &impl ObjectStore,
    settings: &BackupSettings,
    now: DateTime<Utc>,
) -> Result<String, BackupError> {
    let key = format!(
        "{}{}{KEY_SUFFIX}",
        settings.prefix,
        now.format(KEY_TIMESTAMP)
    );
    let archive = snapshot(database, &settings.work_dir).await?;
    let uploaded = store.put(&key, &archive).await;
    remove_if_present(&archive);
    uploaded?;
    tracing::info!(key = %key, "database backup uploaded");
    if let Err(error) = prune(store, &settings.prefix, settings.retention).await {
        tracing::error!(error = %error, "backup retention pruning failed");
    }
    Ok(key)
}

/// Writes a consistent `VACUUM INTO` snapshot and returns its gzip archive path.
async fn snapshot(database: &Database, work_dir: &Path) -> Result<PathBuf, BackupError> {
    fs::create_dir_all(work_dir).map_err(|source| BackupError::Io {
        path: work_dir.to_owned(),
        source,
    })?;
    let name = Uuid::new_v4().to_string();
    let snapshot = work_dir.join(format!("{name}.db"));
    let archive = work_dir.join(format!("{name}{KEY_SUFFIX}"));
    let vacuumed = vacuum_into(database, &snapshot).await;
    if let Err(error) = vacuumed {
        remove_database_files(&snapshot);
        return Err(error);
    }
    let compressed = {
        let snapshot = snapshot.clone();
        let archive = archive.clone();
        tokio::task::spawn_blocking(move || gzip(&snapshot, &archive))
            .await
            .map_err(BackupError::Task)?
    };
    remove_database_files(&snapshot);
    if compressed.is_err() {
        remove_if_present(&archive);
    }
    compressed.map(|()| archive)
}

/// Copies the live database into a new file through the shared handle.
async fn vacuum_into(database: &Database, snapshot: &Path) -> Result<(), BackupError> {
    let target = snapshot.to_string_lossy().replace('\'', "''");
    let connection = database.connection().await.map_err(database_error)?;
    connection
        .execute(&format!("VACUUM INTO '{target}'"), ())
        .await
        .map_err(database_error)?;
    Ok(())
}

/// Gzip-compresses one file into another.
fn gzip(source: &Path, target: &Path) -> Result<(), BackupError> {
    let io_error = |path: &Path| {
        let path = path.to_owned();
        move |source| BackupError::Io { path, source }
    };
    let mut input = fs::File::open(source).map_err(io_error(source))?;
    let output = fs::File::create(target).map_err(io_error(target))?;
    let mut encoder = GzEncoder::new(output, Compression::default());
    io::copy(&mut input, &mut encoder).map_err(io_error(target))?;
    encoder
        .finish()
        .and_then(|file| file.sync_all())
        .map_err(io_error(target))
}

/// Deletes the oldest Webstack backups under `prefix` beyond `retention`.
async fn prune(
    store: &impl ObjectStore,
    prefix: &str,
    retention: usize,
) -> Result<(), BackupError> {
    let mut keys: Vec<String> = store
        .list(prefix)
        .await?
        .into_iter()
        .filter(|key| is_backup_key(prefix, key))
        .collect();
    keys.sort();
    let excess = keys.len().saturating_sub(retention);
    for key in keys.into_iter().take(excess) {
        store.delete(&key).await?;
        tracing::info!(key = %key, "pruned database backup");
    }
    Ok(())
}

/// Reports whether a key is a timestamped Webstack backup directly under `prefix`.
fn is_backup_key(prefix: &str, key: &str) -> bool {
    key.strip_prefix(prefix)
        .and_then(|name| name.strip_suffix(KEY_SUFFIX))
        .is_some_and(|stamp| {
            stamp.len() == KEY_TIMESTAMP_LENGTH
                && stamp.bytes().enumerate().all(|(index, byte)| match index {
                    8 => byte == b'T',
                    15 => byte == b'Z',
                    _ => byte.is_ascii_digit(),
                })
        })
}

/// Removes a database file and any sidecar journal files.
pub(crate) fn remove_database_files(path: &Path) {
    remove_if_present(path);
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(suffix);
        remove_if_present(Path::new(&sidecar));
    }
}

/// Deletes a file, logging failures other than absence.
fn remove_if_present(path: &Path) {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(path = %path.display(), %error, "cannot remove temporary backup file");
    }
}

/// Wraps a Turso error.
fn database_error(source: turso::Error) -> BackupError {
    BackupError::Database(Box::new(source))
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use chrono::{TimeZone, Utc};
    use flate2::read::GzDecoder;
    use tempfile::tempdir;

    use tokio::sync::Mutex;

    use super::{BackupSettings, Backups, back_up, is_backup_key};
    use crate::store::{
        R2Store,
        tests::{MemoryStore, mock_s3, r2_config},
    };

    async fn seeded_database(path: &std::path::Path) -> webstack_db::Database {
        let database = webstack_db::connect(path).await.expect("database");
        let connection = database.connection().await.expect("connection");
        connection
            .execute("CREATE TABLE note (id INTEGER PRIMARY KEY, body TEXT)", ())
            .await
            .expect("table");
        for index in 0..50 {
            connection
                .execute(
                    "INSERT INTO note (body) VALUES (?1)",
                    (format!("note {index}"),),
                )
                .await
                .expect("row");
        }
        database
    }

    #[test]
    fn only_timestamped_backups_under_the_prefix_are_recognized() {
        assert!(is_backup_key("db/", "db/20261024T030000Z.db.gz"));
        assert!(!is_backup_key("db/", "db/nested/20261024T030000Z.db.gz"));
        assert!(!is_backup_key("db/", "db/manual-copy.db.gz"));
        assert!(!is_backup_key("db/", "db/20261024T030000Z.db"));
        assert!(!is_backup_key("db/", "other/20261024T030000Z.db.gz"));
    }

    #[tokio::test]
    async fn backups_run_through_the_s3_sdk_and_prune_by_listing() {
        let data = tempdir().expect("data directory");
        let (endpoint, objects) = mock_s3().await;
        let backups = Backups {
            database: seeded_database(&data.path().join("app.db")).await,
            store: R2Store::with_endpoint(&r2_config("backups"), &endpoint).expect("R2 store"),
            settings: BackupSettings {
                prefix: "db-backups/".to_owned(),
                retention: 1,
                work_dir: data.path().join(".webstack-backup"),
            },
            running: Mutex::new(()),
        };
        objects.lock().expect("objects lock").insert(
            "db-backups/20200101T000000Z.db.gz".to_owned(),
            b"old".to_vec(),
        );

        let key = backups.run().await.expect("backup");

        let objects = objects.lock().expect("objects lock");
        assert_eq!(objects.keys().collect::<Vec<_>>(), vec![&key]);
        assert!(objects[&key].starts_with(&[0x1f, 0x8b]), "gzip archive");
    }

    #[tokio::test]
    async fn backups_upload_a_gzipped_consistent_snapshot_and_keep_the_newest() {
        let data = tempdir().expect("data directory");
        let database = seeded_database(&data.path().join("app.db")).await;
        let store = MemoryStore::default();
        store
            .objects
            .lock()
            .expect("objects lock")
            .insert("db/keep-me.txt".to_owned(), b"foreign".to_vec());
        let settings = BackupSettings {
            prefix: "db/".to_owned(),
            retention: 2,
            work_dir: data.path().join(".webstack-backup"),
        };

        let mut keys = Vec::new();
        for day in 1..=3 {
            let now = Utc.with_ymd_and_hms(2026, 10, day, 3, 0, 0).unwrap();
            keys.push(
                back_up(&database, &store, &settings, now)
                    .await
                    .expect("backup"),
            );
        }

        assert_eq!(keys[2], "db/20261003T030000Z.db.gz");
        let objects = store.objects.lock().expect("objects lock").clone();
        assert_eq!(
            objects.keys().collect::<Vec<_>>(),
            vec![
                "db/20261002T030000Z.db.gz",
                "db/20261003T030000Z.db.gz",
                "db/keep-me.txt"
            ]
        );
        assert_eq!(
            std::fs::read_dir(&settings.work_dir)
                .expect("work directory")
                .count(),
            0,
            "temporary snapshot files are removed"
        );

        let mut snapshot = Vec::new();
        GzDecoder::new(objects["db/20261003T030000Z.db.gz"].as_slice())
            .read_to_end(&mut snapshot)
            .expect("gzip snapshot");
        let restored = data.path().join("restored.db");
        std::fs::write(&restored, snapshot).expect("snapshot file");
        let copy = webstack_db::connect(&restored)
            .await
            .expect("snapshot database");
        let connection = copy.connection().await.expect("snapshot connection");
        let mut rows = connection
            .query("SELECT count(*) FROM note", ())
            .await
            .expect("count query");
        let count: i64 = rows
            .next()
            .await
            .expect("count row")
            .expect("count value")
            .get(0)
            .expect("count");
        assert_eq!(count, 50);
    }
}
