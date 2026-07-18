use askama::Template;
use webstack::ErrorView;

#[derive(Template)]
#[template(path = "errors/page.html.jinja", ext = "html")]
struct ErrorPageTemplate<'a> {
    error: &'a ErrorView,
}

#[derive(Template)]
#[template(path = "errors/partial.html.jinja", ext = "html")]
struct ErrorPartialTemplate<'a> {
    error: &'a ErrorView,
}

/// Renders safe demo errors as a complete page or htmx alert fragment.
pub(crate) fn render(error: &ErrorView) -> Option<String> {
    if error.is_htmx() {
        ErrorPartialTemplate { error }.render().ok()
    } else {
        ErrorPageTemplate { error }.render().ok()
    }
}
