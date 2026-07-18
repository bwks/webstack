use askama::Template;
use askama_web::WebTemplate;

use crate::{domain::items::Item, web::navigation::Navigation};

#[derive(Template, WebTemplate)]
#[template(path = "items/index.html.jinja", ext = "html")]
pub(super) struct ItemsPageTemplate {
    pub(super) nav: Navigation,
    pub(super) csrf_token: String,
    pub(super) items: Vec<Item>,
}

#[derive(Template, WebTemplate)]
#[template(path = "items/region.html.jinja", ext = "html")]
pub(super) struct ItemsRegionTemplate {
    pub(super) csrf_token: String,
    pub(super) items: Vec<Item>,
}

#[derive(Template, WebTemplate)]
#[template(path = "items/edit.html.jinja", ext = "html")]
pub(super) struct ItemEditPageTemplate {
    pub(super) nav: Navigation,
    pub(super) csrf_token: String,
    pub(super) item: Item,
}

#[derive(Template, WebTemplate)]
#[template(path = "items/edit_form.html.jinja", ext = "html")]
pub(super) struct ItemEditPartialTemplate {
    pub(super) csrf_token: String,
    pub(super) item: Item,
}

#[cfg(test)]
mod tests {
    use askama::Template;

    use super::ItemsRegionTemplate;
    use crate::domain::items::Item;

    #[test]
    fn items_region_keeps_swap_targets_csrf_and_escaped_content() {
        let html = ItemsRegionTemplate {
            csrf_token: "safe-token".to_owned(),
            items: vec![Item {
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
