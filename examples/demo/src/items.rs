use askama::Template;
use askama_web::WebTemplate;
use webstack::{
    AppError, AppState, ApplicationBuilder,
    auth::{AuthSession, CsrfToken},
    axum::{
        Form,
        extract::{Path, State},
        response::{IntoResponse, Redirect, Response},
        routing::{get, post, put},
    },
    database::retry_write,
    htmx::HxRequest,
    serde::Deserialize,
    surrealdb::types::{RecordId, RecordIdKey, SurrealValue},
};

use crate::{Navigation, navigation};

#[derive(Clone, Debug)]
struct ItemView {
    id: String,
    name: String,
}

#[derive(Debug, SurrealValue)]
#[surreal(crate = "webstack::surrealdb::types")]
struct ItemRecord {
    id: RecordId,
    name: String,
}

impl ItemRecord {
    /// Converts a database item into template-safe application data.
    fn into_view(self) -> Result<ItemView, AppError> {
        let RecordIdKey::String(id) = self.id.key else {
            return Err(AppError::internal(
                "read item identifier",
                std::io::Error::other("item identifier was not a string"),
            ));
        };
        Ok(ItemView {
            id,
            name: self.name,
        })
    }
}

#[derive(Deserialize)]
#[serde(crate = "webstack::serde")]
struct ItemForm {
    name: String,
}

impl ItemForm {
    /// Normalizes and validates the submitted demo item name.
    fn name(&self) -> Result<String, AppError> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(AppError::validation(
                "Item names must contain between 1 and 100 characters.",
            ));
        }
        Ok(name.to_owned())
    }
}

#[derive(Template, WebTemplate)]
#[template(path = "pages/items.html.jinja", ext = "html")]
struct ItemsPageTemplate {
    nav: Navigation,
    csrf_token: String,
    items: Vec<ItemView>,
}

#[derive(Template, WebTemplate)]
#[template(path = "partials/items_region.html.jinja", ext = "html")]
struct ItemsRegionTemplate {
    csrf_token: String,
    items: Vec<ItemView>,
}

#[derive(Template, WebTemplate)]
#[template(path = "pages/item_edit.html.jinja", ext = "html")]
struct ItemEditPageTemplate {
    nav: Navigation,
    csrf_token: String,
    item: ItemView,
}

#[derive(Template, WebTemplate)]
#[template(path = "partials/item_edit.html.jinja", ext = "html")]
struct ItemEditPartialTemplate {
    csrf_token: String,
    item: ItemView,
}

/// Adds the shared Items reference feature to the demo application.
pub(crate) fn routes(
    application: ApplicationBuilder,
) -> Result<ApplicationBuilder, webstack::ApplicationError> {
    application
        .authenticated_route("/items", get(index))?
        .role_route("/items", "user", post(create))?
        .authenticated_route("/items/{id}/edit", get(edit))?
        .role_route(
            "/items/{id}",
            "user",
            put(update).delete(remove).post(update),
        )?
        .role_route("/items/{id}/delete", "user", post(remove))
}

/// Renders the full Items page or its stable htmx region.
async fn index(
    State(state): State<AppState>,
    auth: AuthSession,
    htmx: HxRequest,
    csrf: CsrfToken,
) -> Result<Response, AppError> {
    let items = load_items(&state).await?;
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
async fn create(
    State(state): State<AppState>,
    htmx: HxRequest,
    csrf: CsrfToken,
    Form(form): Form<ItemForm>,
) -> Result<Response, AppError> {
    let name = form.name()?;
    let database = state.database().clone();
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
    .map_err(|source| AppError::internal("create item", source))?;
    mutation_response(&state, htmx, &csrf).await
}

/// Renders an item's full-page or partial edit form.
async fn edit(
    State(state): State<AppState>,
    Path(id): Path<String>,
    auth: AuthSession,
    htmx: HxRequest,
    csrf: CsrfToken,
) -> Result<Response, AppError> {
    let item = load_item(&state, &id).await?.into_view()?;
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
async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    htmx: HxRequest,
    csrf: CsrfToken,
    Form(form): Form<ItemForm>,
) -> Result<Response, AppError> {
    let name = form.name()?;
    let database = state.database().clone();
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
    if found.is_none() {
        return Err(AppError::not_found("The requested item does not exist."));
    }
    mutation_response(&state, htmx, &csrf).await
}

/// Deletes one shared demo item.
async fn remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
    htmx: HxRequest,
    csrf: CsrfToken,
) -> Result<Response, AppError> {
    let database = state.database().clone();
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
    if found.is_none() {
        return Err(AppError::not_found("The requested item does not exist."));
    }
    mutation_response(&state, htmx, &csrf).await
}

/// Renders a mutation result for htmx or redirects an ordinary browser.
async fn mutation_response(
    state: &AppState,
    htmx: HxRequest,
    csrf: &CsrfToken,
) -> Result<Response, AppError> {
    if htmx.is_htmx() {
        let items = load_items(state).await?;
        Ok(ItemsRegionTemplate {
            csrf_token: csrf.as_str().to_owned(),
            items,
        }
        .into_response())
    } else {
        Ok(Redirect::to("/items").into_response())
    }
}

/// Loads every demo item in creation order.
async fn load_items(state: &AppState) -> Result<Vec<ItemView>, AppError> {
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
        .map(ItemRecord::into_view)
        .collect()
}

/// Loads one demo item or returns a safe not-found error.
async fn load_item(state: &AppState, id: &str) -> Result<ItemRecord, AppError> {
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
        .ok_or_else(|| AppError::not_found("The requested item does not exist."))
}

#[cfg(test)]
mod tests {
    use askama::Template;

    use super::{ItemView, ItemsRegionTemplate};

    #[test]
    fn items_region_keeps_swap_targets_csrf_and_escaped_content() {
        let html = ItemsRegionTemplate {
            csrf_token: "safe-token".to_owned(),
            items: vec![ItemView {
                id: "first".to_owned(),
                name: "<script>alert(1)</script>".to_owned(),
            }],
        }
        .render()
        .expect("items region");

        assert!(html.contains("id=\"items-region\""));
        assert!(html.contains("id=\"item-errors\""));
        assert!(html.contains("X-CSRF-Token"));
        assert!(html.contains("safe-token"));
        assert!(html.contains("&#60;script&#62;alert(1)&#60;/script&#62;"));
        assert!(!html.contains("<script>alert(1)</script>"));
    }
}
