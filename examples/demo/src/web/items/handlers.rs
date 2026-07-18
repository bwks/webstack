use webstack::{
    AppError, AppState,
    auth::{AuthSession, CsrfToken},
    axum::{
        Form,
        extract::{Path, State},
        response::{IntoResponse, Redirect, Response},
    },
    htmx::HxRequest,
    serde::Deserialize,
};

use crate::{domain::items, web::navigation::navigation};

use super::views::{
    ItemEditPageTemplate, ItemEditPartialTemplate, ItemsPageTemplate, ItemsRegionTemplate,
};

#[derive(Deserialize)]
#[serde(crate = "webstack::serde")]
pub(super) struct ItemForm {
    name: String,
}

/// Renders the full Items page or its stable htmx region.
pub(super) async fn index(
    State(state): State<AppState>,
    auth: AuthSession,
    htmx: HxRequest,
    csrf: CsrfToken,
) -> Result<Response, AppError> {
    let items = items::list(&state).await?;
    if htmx.is_htmx() {
        Ok(ItemsRegionTemplate {
            csrf_token: csrf.as_str().to_owned(),
            items,
        }
        .into_response())
    } else {
        Ok(ItemsPageTemplate {
            nav: navigation(&auth, &csrf),
            csrf_token: csrf.as_str().to_owned(),
            items,
        }
        .into_response())
    }
}

/// Creates a shared demo item.
pub(super) async fn create(
    State(state): State<AppState>,
    htmx: HxRequest,
    csrf: CsrfToken,
    Form(form): Form<ItemForm>,
) -> Result<Response, AppError> {
    items::create(&state, &form.name).await?;
    mutation_response(&state, htmx, &csrf).await
}

/// Renders an item's full-page or partial edit form.
pub(super) async fn edit(
    State(state): State<AppState>,
    Path(id): Path<String>,
    auth: AuthSession,
    htmx: HxRequest,
    csrf: CsrfToken,
) -> Result<Response, AppError> {
    let item = items::get(&state, &id).await?;
    if htmx.is_htmx() {
        Ok(ItemEditPartialTemplate {
            csrf_token: csrf.as_str().to_owned(),
            item,
        }
        .into_response())
    } else {
        Ok(ItemEditPageTemplate {
            nav: navigation(&auth, &csrf),
            csrf_token: csrf.as_str().to_owned(),
            item,
        }
        .into_response())
    }
}

/// Updates one shared demo item.
pub(super) async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    htmx: HxRequest,
    csrf: CsrfToken,
    Form(form): Form<ItemForm>,
) -> Result<Response, AppError> {
    items::update(&state, &id, &form.name).await?;
    mutation_response(&state, htmx, &csrf).await
}

/// Deletes one shared demo item.
pub(super) async fn remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
    htmx: HxRequest,
    csrf: CsrfToken,
) -> Result<Response, AppError> {
    items::delete(&state, &id).await?;
    mutation_response(&state, htmx, &csrf).await
}

/// Renders a mutation result for htmx or redirects an ordinary browser.
async fn mutation_response(
    state: &AppState,
    htmx: HxRequest,
    csrf: &CsrfToken,
) -> Result<Response, AppError> {
    if htmx.is_htmx() {
        let items = items::list(state).await?;
        Ok(ItemsRegionTemplate {
            csrf_token: csrf.as_str().to_owned(),
            items,
        }
        .into_response())
    } else {
        Ok(Redirect::to("/items").into_response())
    }
}
