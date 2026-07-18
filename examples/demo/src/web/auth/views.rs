use askama::Template;
use askama_web::WebTemplate;

use crate::web::navigation::Navigation;

#[derive(Template, WebTemplate)]
#[template(path = "auth/login.html.jinja", ext = "html")]
pub(crate) struct LoginTemplate {
    pub(crate) nav: Navigation,
    pub(crate) message: &'static str,
}

#[derive(Template, WebTemplate)]
#[template(path = "auth/change_password.html.jinja", ext = "html")]
pub(crate) struct PasswordTemplate {
    pub(crate) nav: Navigation,
    pub(crate) message: &'static str,
}
