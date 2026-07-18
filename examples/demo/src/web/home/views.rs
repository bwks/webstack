use askama::Template;
use askama_web::WebTemplate;

use crate::web::navigation::Navigation;

#[derive(Template, WebTemplate)]
#[template(path = "home/index.html.jinja", ext = "html")]
pub(crate) struct IndexTemplate {
    pub(crate) nav: Navigation,
}

#[derive(Template, WebTemplate)]
#[template(path = "account/index.html.jinja", ext = "html")]
pub(crate) struct AccountTemplate {
    pub(crate) nav: Navigation,
    pub(crate) roles: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "admin/index.html.jinja", ext = "html")]
pub(crate) struct AdminTemplate {
    pub(crate) nav: Navigation,
}

#[cfg(test)]
mod tests {
    use askama::Template;

    use super::IndexTemplate;
    use crate::web::navigation::Navigation;

    #[test]
    fn anonymous_home_has_theme_controls_and_sign_in_action() {
        let html = IndexTemplate {
            nav: Navigation {
                authenticated: false,
                username: String::new(),
                is_admin: false,
                csrf_token: "csrf".to_owned(),
            },
        }
        .render()
        .expect("anonymous home");

        assert!(html.contains("data-theme-toggle"));
        assert!(html.contains("/static/css/app.css"));
        assert!(html.contains("/static/js/theme.js"));
        assert!(html.contains(
            r#"href="/static/images/favicon-light.png" media="(prefers-color-scheme: light)""#
        ));
        assert!(html.contains(
            r#"href="/static/images/favicon-dark.png" media="(prefers-color-scheme: dark)""#
        ));
        assert!(html.contains(r#"data-favicon-theme="light""#));
        assert!(html.contains(r#"data-favicon-theme="dark""#));
        assert!(html.contains("Sign in to the demo"));
        assert!(!html.contains("action=\"/logout\""));
    }

    #[test]
    fn administrator_home_has_authenticated_navigation_and_logout_csrf() {
        let html = IndexTemplate {
            nav: Navigation {
                authenticated: true,
                username: "admin".to_owned(),
                is_admin: true,
                csrf_token: "safe-token".to_owned(),
            },
        }
        .render()
        .expect("administrator home");

        assert!(html.contains("href=\"/items\""));
        assert!(html.contains("href=\"/admin\""));
        assert!(html.contains("action=\"/logout\""));
        assert!(html.contains("value=\"safe-token\""));
    }
}
