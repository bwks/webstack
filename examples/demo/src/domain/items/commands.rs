use webstack::{AppError, AppState, database::retry_write};

use super::queries::ItemRecord;

/// Creates a shared demo item inside the framework's retry boundary.
pub(super) async fn create(state: &AppState, name: &str) -> Result<(), AppError> {
    let database = state.database().clone();
    let name = name.to_owned();
    retry_write(|| {
        let database = database.clone();
        let name = name.clone();
        async move {
            database
                .query("CREATE item SET name = $name;")
                .bind(("name", name))
                .await?
                .check()?;
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
    let found = retry_write(|| {
        let database = database.clone();
        let id = id.clone();
        let name = name.clone();
        async move {
            let mut response = database
                .query("UPDATE ONLY type::record('item', $id) SET name = $name RETURN id, name;")
                .bind(("id", id))
                .bind(("name", name))
                .await?
                .check()?;
            response.take::<Option<ItemRecord>>(0)
        }
    })
    .await
    .map_err(|source| AppError::internal("update item", source))?;
    found
        .ok_or_else(|| AppError::not_found("The requested item does not exist."))?
        .into_item()?;
    Ok(())
}

/// Deletes a shared demo item inside the framework's retry boundary.
pub(super) async fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
    let database = state.database().clone();
    let id = id.to_owned();
    let found = retry_write(|| {
        let database = database.clone();
        let id = id.clone();
        async move {
            let mut response = database
                .query("DELETE ONLY type::record('item', $id) RETURN BEFORE;")
                .bind(("id", id))
                .await?
                .check()?;
            response.take::<Option<ItemRecord>>(0)
        }
    })
    .await
    .map_err(|source| AppError::internal("delete item", source))?;
    found
        .ok_or_else(|| AppError::not_found("The requested item does not exist."))?
        .into_item()?;
    Ok(())
}
