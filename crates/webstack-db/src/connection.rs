use std::path::Path;

use surrealdb::{
    Surreal,
    engine::local::{Db, RocksDb},
};

use crate::error::DatabaseError;

/// The one embedded `SurrealDB` handle shared by a Webstack application.
pub type Database = Surreal<Db>;

/// Opens the configured `RocksDB` directory and selects its namespace and database.
///
/// # Errors
///
/// Returns a typed connection or selection error retaining `SurrealDB`'s source.
pub async fn connect(
    path: &Path,
    namespace: &str,
    database: &str,
) -> Result<Database, DatabaseError> {
    let db = Surreal::new::<RocksDb>(path)
        .await
        .map_err(|source| DatabaseError::Connect {
            path: path.to_owned(),
            source: Box::new(source),
        })?;
    db.use_ns(namespace)
        .use_db(database)
        .await
        .map_err(|source| DatabaseError::Select {
            namespace: namespace.to_owned(),
            database: database.to_owned(),
            source: Box::new(source),
        })?;
    Ok(db)
}
