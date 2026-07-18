mod commands;
mod item;
mod queries;

use webstack::{AppError, AppState};

pub(crate) use item::Item;

/// Lists all demo items in stable creation order.
pub(crate) async fn list(state: &AppState) -> Result<Vec<Item>, AppError> {
    queries::list(state).await
}

/// Loads one demo item by its string record identifier.
pub(crate) async fn get(state: &AppState, id: &str) -> Result<Item, AppError> {
    queries::get(state, id).await
}

/// Validates and creates a shared demo item.
pub(crate) async fn create(state: &AppState, name: &str) -> Result<(), AppError> {
    let name = Item::normalize_name(name)?;
    commands::create(state, &name).await
}

/// Validates and updates an existing shared demo item.
pub(crate) async fn update(state: &AppState, id: &str, name: &str) -> Result<(), AppError> {
    let name = Item::normalize_name(name)?;
    commands::update(state, id, &name).await
}

/// Deletes an existing shared demo item.
pub(crate) async fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
    commands::delete(state, id).await
}
