use webstack::{AppError, AppState};

use super::Item;

/// One item row decoded from Turso.
struct ItemRecord {
    id: String,
    name: String,
}

impl ItemRecord {
    /// Decodes a Turso row into an item record.
    fn from_row(row: &webstack::turso::Row) -> Result<Self, AppError> {
        Ok(Self {
            id: row
                .get(0)
                .map_err(|source| AppError::internal("decode item identifier", source))?,
            name: row
                .get(1)
                .map_err(|source| AppError::internal("decode item name", source))?,
        })
    }

    /// Converts a database record into an application item.
    fn into_item(self) -> Item {
        Item::from_parts(self.id, self.name)
    }
}

/// Loads every demo item in stable creation order.
pub(super) async fn list(state: &AppState) -> Result<Vec<Item>, AppError> {
    let connection = state
        .database()
        .connection()
        .await
        .map_err(|source| AppError::internal("connect to load items", source))?;
    let mut rows = connection
        .query("SELECT id, name FROM item ORDER BY created_at, id", ())
        .await
        .map_err(|source| AppError::internal("load items", source))?;
    let mut items = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|source| AppError::internal("load items", source))?
    {
        items.push(ItemRecord::from_row(&row)?.into_item());
    }
    Ok(items)
}

/// Loads one demo item or returns a safe not-found error.
pub(super) async fn get(state: &AppState, id: &str) -> Result<Item, AppError> {
    let connection = state
        .database()
        .connection()
        .await
        .map_err(|source| AppError::internal("connect to load item", source))?;
    let mut rows = connection
        .query("SELECT id, name FROM item WHERE id = ?1", (id,))
        .await
        .map_err(|source| AppError::internal("load item", source))?;
    rows.next()
        .await
        .map_err(|source| AppError::internal("load item", source))?
        .map(|row| ItemRecord::from_row(&row).map(ItemRecord::into_item))
        .transpose()?
        .ok_or_else(|| AppError::not_found("The requested item does not exist."))
}
