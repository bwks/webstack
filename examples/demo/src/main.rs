mod errors;
mod items;

use askama::Template;
use askama_web::WebTemplate;
use webstack::{
    Application,
    auth::{AuthMessage, AuthSession, CsrfToken, LoginPageContext, PasswordChangePageContext},
    axum::routing::get,
};

#[derive(rust_embed::RustEmbed)]
#[folder = "assets/"]
struct Assets;

#[derive(Clone)]
struct Navigation {
    authenticated: bool,
    username: String,
    is_admin: bool,
    csrf_token: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "pages/index.html.jinja", ext = "html")]
struct IndexTemplate {
    nav: Navigation,
}

#[derive(Template, WebTemplate)]
#[template(path = "pages/login.html.jinja", ext = "html")]
struct LoginTemplate {
    nav: Navigation,
    message: &'static str,
}

#[derive(Template, WebTemplate)]
#[template(path = "pages/change_password.html.jinja", ext = "html")]
struct PasswordTemplate {
    nav: Navigation,
    message: &'static str,
}

#[derive(Template, WebTemplate)]
#[template(path = "pages/account.html.jinja", ext = "html")]
struct AccountTemplate {
    nav: Navigation,
    roles: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "pages/admin.html.jinja", ext = "html")]
struct AdminTemplate {
    nav: Navigation,
}

/// Renders the demonstration application's public home page.
async fn index(auth: AuthSession, csrf: CsrfToken) -> IndexTemplate {
    IndexTemplate {
        nav: navigation(&auth, &csrf),
    }
}

/// Renders the application-owned login page.
async fn login(context: LoginPageContext, auth: AuthSession) -> LoginTemplate {
    LoginTemplate {
        nav: navigation(&auth, &context.csrf_token),
        message: auth_message(context.message),
    }
}

/// Renders the application-owned password-change page.
async fn change_password(
    context: PasswordChangePageContext,
    auth: AuthSession,
) -> PasswordTemplate {
    PasswordTemplate {
        nav: navigation(&auth, &context.csrf_token),
        message: auth_message(context.message),
    }
}

/// Renders the authenticated account summary.
async fn account(auth: AuthSession, csrf: CsrfToken) -> AccountTemplate {
    let roles = auth
        .user
        .as_ref()
        .map_or_else(String::new, |user| user.roles().join(", "));
    AccountTemplate {
        nav: navigation(&auth, &csrf),
        roles,
    }
}

/// Renders the administrator-only demonstration page.
async fn admin(auth: AuthSession, csrf: CsrfToken) -> AdminTemplate {
    AdminTemplate {
        nav: navigation(&auth, &csrf),
    }
}

/// Builds navigation display state from the current session and CSRF token.
fn navigation(auth: &AuthSession, csrf: &CsrfToken) -> Navigation {
    let user = auth.user.as_ref();
    Navigation {
        authenticated: user.is_some(),
        username: user.map_or_else(String::new, |user| user.username().to_owned()),
        is_admin: user.is_some_and(|user| user.has_role("admin")),
        csrf_token: csrf.as_str().to_owned(),
    }
}

/// Maps authentication state to the demo application's display text.
fn auth_message(message: Option<AuthMessage>) -> &'static str {
    match message {
        Some(AuthMessage::InvalidCredentials) => "The username or password was not accepted.",
        Some(AuthMessage::PasswordExpired) => "Change your password to continue.",
        Some(AuthMessage::PasswordMismatch) => "The new passwords do not match.",
        Some(AuthMessage::PasswordLength) => "Passwords must contain 12 to 128 characters.",
        Some(AuthMessage::PasswordUnchanged) => {
            "Choose a password different from the current password."
        }
        Some(AuthMessage::PasswordChanged) => "Password changed. Sign in again.",
        None => "",
    }
}

#[webstack::tokio::main(crate = "webstack::tokio")]
/// Composes and runs the demonstration application.
async fn main() -> anyhow::Result<()> {
    let application = Application::builder()
        .assets::<Assets>()?
        .error_renderer(errors::render)?
        .auth_pages(get(login), get(change_password))?
        .route("/", get(index))?
        .authenticated_route("/account", get(account))?
        .role_route("/admin", "admin", get(admin))?;
    items::routes(application)?.run().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use askama::Template;

    use super::{IndexTemplate, Navigation};

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
