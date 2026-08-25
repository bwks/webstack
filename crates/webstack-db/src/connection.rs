use std::{fmt, fs, path::Path, time::Duration};

use crate::error::DatabaseError;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// The one embedded Turso database handle shared by a Webstack application.
#[derive(Clone)]
pub struct Database {
    inner: turso::Database,
}

impl Database {
    /// Opens a configured connection with foreign keys and bounded lock waiting enabled.
    ///
    /// # Errors
    ///
    /// Returns a Turso error when the connection or its required pragmas cannot be configured.
    pub async fn connection(&self) -> turso::Result<turso::Connection> {
        let connection = self.inner.connect()?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection.execute("PRAGMA foreign_keys = ON", ()).await?;
        Ok(connection)
    }
}

impl fmt::Debug for Database {
    /// Formats the database handle without exposing internal engine state.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Database").finish()
    }
}

/// Opens the configured embedded Turso database file.
///
/// # Errors
///
/// Returns a typed filesystem, connection, or configuration error retaining its source.
pub async fn connect(path: &Path) -> Result<Database, DatabaseError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| DatabaseError::CreateParent {
            path: parent.to_owned(),
            source,
        })?;
    }
    let path_text = path.to_string_lossy();
    let inner = turso::Builder::new_local(&path_text)
        .build()
        .await
        .map_err(|source| DatabaseError::Connect {
            path: path.to_owned(),
            source: Box::new(source),
        })?;
    let database = Database { inner };
    let connection = database
        .connection()
        .await
        .map_err(|source| DatabaseError::Configure {
            path: path.to_owned(),
            source: Box::new(source),
        })?;
    connection
        .pragma_update("journal_mode", "WAL")
        .await
        .map_err(|source| DatabaseError::Configure {
            path: path.to_owned(),
            source: Box::new(source),
        })?;
    connection
        .pragma_update("synchronous", "FULL")
        .await
        .map_err(|source| DatabaseError::Configure {
            path: path.to_owned(),
            source: Box::new(source),
        })?;
    Ok(database)
}
