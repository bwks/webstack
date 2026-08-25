use async_trait::async_trait;
use tower_sessions::{
    SessionStore,
    session::{Id, Record},
    session_store::{Error, ExpiredDeletion, Result},
};
use webstack_db::Database;

/// A `tower-sessions` store backed by the application's shared Turso database.
#[derive(Clone, Debug)]
pub struct TursoSessionStore {
    database: Database,
}

impl TursoSessionStore {
    /// Creates a session store from the shared database handle.
    #[must_use]
    pub const fn new(database: Database) -> Self {
        Self { database }
    }
}

#[async_trait]
impl SessionStore for TursoSessionStore {
    /// Creates a session while regenerating colliding random IDs.
    async fn create(&self, record: &mut Record) -> Result<()> {
        for _attempt in 0..4 {
            let connection = self
                .database
                .connection()
                .await
                .map_err(|error| backend_error(&error))?;
            let encoded = encode_record(record)?;
            let result = connection
                .execute(
                    "INSERT INTO _webstack_session (id, payload, expires_at) VALUES (?1, ?2, ?3)",
                    (
                        record.id.to_string(),
                        encoded,
                        record.expiry_date.unix_timestamp(),
                    ),
                )
                .await;
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
        let connection = self
            .database
            .connection()
            .await
            .map_err(|error| backend_error(&error))?;
        connection
            .execute(
                "INSERT INTO _webstack_session (id, payload, expires_at) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(id) DO UPDATE SET payload = excluded.payload, \
                 expires_at = excluded.expires_at",
                (
                    record.id.to_string(),
                    encode_record(record)?,
                    record.expiry_date.unix_timestamp(),
                ),
            )
            .await
            .map_err(|error| backend_error(&error))?;
        Ok(())
    }

    /// Loads a live session record and removes an expired record on sight.
    async fn load(&self, session_id: &Id) -> Result<Option<Record>> {
        let connection = self
            .database
            .connection()
            .await
            .map_err(|error| backend_error(&error))?;
        let mut rows = connection
            .query(
                "SELECT payload, expires_at FROM _webstack_session WHERE id = ?1",
                (session_id.to_string(),),
            )
            .await
            .map_err(|error| backend_error(&error))?;
        let Some(row) = rows.next().await.map_err(|error| backend_error(&error))? else {
            return Ok(None);
        };
        let payload: String = row.get(0).map_err(|error| backend_error(&error))?;
        let expires_at: i64 = row.get(1).map_err(|error| backend_error(&error))?;
        if expires_at <= time::OffsetDateTime::now_utc().unix_timestamp() {
            self.delete(session_id).await?;
            return Ok(None);
        }
        let record: Record =
            serde_json::from_str(&payload).map_err(|error| Error::Decode(error.to_string()))?;
        if record.id != *session_id {
            return Err(Error::Decode(
                "session ID does not match its record".to_owned(),
            ));
        }
        Ok(Some(record))
    }

    /// Deletes one session record.
    async fn delete(&self, session_id: &Id) -> Result<()> {
        let connection = self
            .database
            .connection()
            .await
            .map_err(|error| backend_error(&error))?;
        connection
            .execute(
                "DELETE FROM _webstack_session WHERE id = ?1",
                (session_id.to_string(),),
            )
            .await
            .map_err(|error| backend_error(&error))?;
        Ok(())
    }
}

#[async_trait]
impl ExpiredDeletion for TursoSessionStore {
    /// Deletes all expired session rows.
    async fn delete_expired(&self) -> Result<()> {
        let connection = self
            .database
            .connection()
            .await
            .map_err(|error| backend_error(&error))?;
        connection
            .execute(
                "DELETE FROM _webstack_session WHERE expires_at <= ?1",
                (time::OffsetDateTime::now_utc().unix_timestamp(),),
            )
            .await
            .map_err(|error| backend_error(&error))?;
        Ok(())
    }
}

/// Encodes a session record for durable storage.
fn encode_record(record: &Record) -> Result<String> {
    serde_json::to_string(record).map_err(|error| Error::Encode(error.to_string()))
}

/// Maps a Turso failure into the session-store error surface.
fn backend_error(error: &turso::Error) -> Error {
    Error::Backend(error.to_string())
}

/// Checks whether a failed session creation collided with an existing ID.
async fn session_exists(database: &Database, session_id: &Id) -> Result<bool> {
    let connection = database
        .connection()
        .await
        .map_err(|error| backend_error(&error))?;
    let mut rows = connection
        .query(
            "SELECT EXISTS(SELECT 1 FROM _webstack_session WHERE id = ?1)",
            (session_id.to_string(),),
        )
        .await
        .map_err(|error| backend_error(&error))?;
    let row = rows
        .next()
        .await
        .map_err(|error| backend_error(&error))?
        .ok_or_else(|| Error::Backend("session existence query returned no row".to_owned()))?;
    row.get::<i64>(0)
        .map(|exists| exists != 0)
        .map_err(|error| backend_error(&error))
}
