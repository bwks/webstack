use webstack::{AppError, AppState, database::retry_write};

/// Creates a shared demo item inside the framework's retry boundary.
pub(super) async fn create(state: &AppState, name: &str) -> Result<(), AppError> {
    let database = state.database().clone();
    let name = name.to_owned();
    retry_write(|| {
        let database = database.clone();
        let name = name.clone();
        async move {
            let connection = database.connection().await?;
            connection
                .execute(
                    "INSERT INTO item (id, name) VALUES (lower(hex(randomblob(16))), ?1)",
                    (name,),
                )
                .await?;
            Ok(())
        }
    })
    .await
    .map_err(|source| AppError::internal("create item", source))
}

/// Updates a shared demo item inside the framework's retry boundary.
pub(super) async fn update(state: &AppState, id: &str, name: &str) -> Result<(), AppError> {
    let database = state.database().clone();
    let id = id.to_owned();
    let name = name.to_owned();
    let changed = retry_write(|| {
        let database = database.clone();
        let id = id.clone();
        let name = name.clone();
        async move {
            let connection = database.connection().await?;
            connection
                .execute(
                    "UPDATE item SET name = ?1, updated_at = unixepoch() WHERE id = ?2",
                    (name, id),
                )
                .await
        }
    })
    .await
    .map_err(|source| AppError::internal("update item", source))?;
    if changed == 0 {
        return Err(AppError::not_found("The requested item does not exist."));
    }
    Ok(())
}

/// Deletes a shared demo item inside the framework's retry boundary.
pub(super) async fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
    let database = state.database().clone();
    let id = id.to_owned();
    let changed = retry_write(|| {
        let database = database.clone();
        let id = id.clone();
        async move {
            let connection = database.connection().await?;
            connection
                .execute("DELETE FROM item WHERE id = ?1", (id,))
                .await
        }
    })
    .await
    .map_err(|source| AppError::internal("delete item", source))?;
    if changed == 0 {
        return Err(AppError::not_found("The requested item does not exist."));
    }
    Ok(())
}
