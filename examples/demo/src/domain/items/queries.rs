use webstack::{
    AppError, AppState,
    surrealdb::types::{RecordId, RecordIdKey, SurrealValue},
};

use super::Item;

#[derive(Debug, SurrealValue)]
#[surreal(crate = "webstack::surrealdb::types")]
pub(super) struct ItemRecord {
    id: RecordId,
    name: String,
}

impl ItemRecord {
    /// Converts a database record into an application item.
    pub(super) fn into_item(self) -> Result<Item, AppError> {
        let RecordIdKey::String(id) = self.id.key else {
            return Err(AppError::internal(
                "read item identifier",
                std::io::Error::other("item identifier was not a string"),
            ));
        };
        Ok(Item::from_parts(id, self.name))
    }
}

/// Loads every demo item in stable creation order.
pub(super) async fn list(state: &AppState) -> Result<Vec<Item>, AppError> {
    let mut response = state
        .database()
        .query("SELECT id, name, created_at FROM item ORDER BY created_at, id;")
        .await
        .map_err(|source| AppError::internal("load items", source))?
        .check()
        .map_err(|source| AppError::internal("load items", source))?;
    response
        .take::<Vec<ItemRecord>>(0)
        .map_err(|source| AppError::internal("decode items", source))?
        .into_iter()
        .map(ItemRecord::into_item)
        .collect()
}

/// Loads one demo item or returns a safe not-found error.
pub(super) async fn get(state: &AppState, id: &str) -> Result<Item, AppError> {
    let mut response = state
        .database()
        .query("SELECT id, name FROM ONLY type::record('item', $id);")
        .bind(("id", id.to_owned()))
        .await
        .map_err(|source| AppError::internal("load item", source))?
        .check()
        .map_err(|source| AppError::internal("load item", source))?;
    response
        .take::<Option<ItemRecord>>(0)
        .map_err(|source| AppError::internal("decode item", source))?
        .ok_or_else(|| AppError::not_found("The requested item does not exist."))?
        .into_item()
}
