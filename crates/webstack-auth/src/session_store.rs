use async_trait::async_trait;
use surrealdb::types::SurrealValue;
use tower_sessions::{
    SessionStore,
    session::{Id, Record},
    session_store::{Error, ExpiredDeletion, Result},
};
use webstack_db::Database;

#[derive(SurrealValue)]
struct SessionRow {
    payload: String,
    expires_at: i64,
}

/// A `tower-sessions` store backed by the application's shared `SurrealDB` handle.
#[derive(Clone, Debug)]
pub struct SurrealSessionStore {
    database: Database,
}

impl SurrealSessionStore {
    /// Creates a session store from the shared database handle.
    #[must_use]
    pub const fn new(database: Database) -> Self {
        Self { database }
    }
}

#[async_trait]
impl SessionStore for SurrealSessionStore {
    /// Creates a session while regenerating colliding random IDs.
    async fn create(&self, record: &mut Record) -> Result<()> {
        for _attempt in 0..4 {
            let encoded = encode_record(record)?;
            let result = self
                .database
                .query("CREATE ONLY type::record('_webstack_session', $id) CONTENT { payload: $payload, expires_at: $expires_at };")
                .bind(("id", record.id.to_string()))
                .bind(("payload", encoded))
                .bind(("expires_at", record.expiry_date.unix_timestamp()))
                .await
                .and_then(surrealdb::IndexedResults::check);
            match result {
                Ok(_) => return Ok(()),
                Err(_) if session_exists(&self.database, &record.id).await? => {
                    record.id = Id::default();
                }
                Err(error) => return Err(backend_error(&error)),
            }
        }
        Err(Error::Backend(
            "cannot allocate a unique session ID".to_owned(),
        ))
    }

    /// Saves an existing session record.
    async fn save(&self, record: &Record) -> Result<()> {
        self.database
            .query("UPSERT type::record('_webstack_session', $id) CONTENT { payload: $payload, expires_at: $expires_at };")
            .bind(("id", record.id.to_string()))
            .bind(("payload", encode_record(record)?))
            .bind(("expires_at", record.expiry_date.unix_timestamp()))
            .await
            .and_then(surrealdb::IndexedResults::check)
            .map_err(|error| backend_error(&error))?;
        Ok(())
    }

    /// Loads a live session record and removes an expired record on sight.
    async fn load(&self, session_id: &Id) -> Result<Option<Record>> {
        let mut response = self
            .database
            .query("SELECT payload, expires_at FROM type::record('_webstack_session', $id);")
            .bind(("id", session_id.to_string()))
            .await
            .and_then(surrealdb::IndexedResults::check)
            .map_err(|error| backend_error(&error))?;
        let row: Option<SessionRow> = response.take(0).map_err(|error| backend_error(&error))?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.expires_at <= time::OffsetDateTime::now_utc().unix_timestamp() {
            self.delete(session_id).await?;
            return Ok(None);
        }
        let record: Record =
            serde_json::from_str(&row.payload).map_err(|error| Error::Decode(error.to_string()))?;
        if record.id != *session_id {
            return Err(Error::Decode(
                "session ID does not match its record".to_owned(),
            ));
        }
        Ok(Some(record))
    }

    /// Deletes one session record.
    async fn delete(&self, session_id: &Id) -> Result<()> {
        self.database
            .query("DELETE type::record('_webstack_session', $id);")
            .bind(("id", session_id.to_string()))
            .await
            .and_then(surrealdb::IndexedResults::check)
            .map_err(|error| backend_error(&error))?;
        Ok(())
    }
}

#[async_trait]
impl ExpiredDeletion for SurrealSessionStore {
    /// Deletes all expired session rows.
    async fn delete_expired(&self) -> Result<()> {
        self.database
            .query("DELETE _webstack_session WHERE expires_at <= $now;")
            .bind(("now", time::OffsetDateTime::now_utc().unix_timestamp()))
            .await
            .and_then(surrealdb::IndexedResults::check)
            .map_err(|error| backend_error(&error))?;
        Ok(())
    }
}

/// Encodes a session record for durable storage.
fn encode_record(record: &Record) -> Result<String> {
    serde_json::to_string(record).map_err(|error| Error::Encode(error.to_string()))
}

/// Maps a `SurrealDB` failure into the session-store error surface.
fn backend_error(error: &surrealdb::Error) -> Error {
    Error::Backend(error.to_string())
}

/// Checks whether a failed session creation collided with an existing ID.
async fn session_exists(database: &Database, session_id: &Id) -> Result<bool> {
    let mut response = database
        .query("RETURN record::exists(type::record('_webstack_session', $id));")
        .bind(("id", session_id.to_string()))
        .await
        .and_then(surrealdb::IndexedResults::check)
        .map_err(|error| backend_error(&error))?;
    response
        .take::<Option<bool>>(0)
        .map(|exists| exists.unwrap_or(false))
        .map_err(|error| backend_error(&error))
}
