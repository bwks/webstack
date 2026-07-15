use webstack::auth::AuthMessage;
use webstack::axum::{response::Html, routing::get};
use webstack::prelude::*;

/// Renders the demo application's root response.
async fn index() -> &'static str {
    "Webstack demo"
}

/// Renders the demo login form owned by the example application.
async fn login(context: LoginPageContext) -> Html<String> {
    Html(format!(
        "<h1>Sign in</h1><p>{}</p><form method=\"post\" action=\"/login\"><input type=\"hidden\" name=\"_csrf\" value=\"{}\"><input name=\"username\"><input type=\"password\" name=\"password\"><button>Sign in</button></form>",
        auth_message(context.message),
        context.csrf_token.as_str()
    ))
}

/// Renders the demo password-change form owned by the example application.
async fn change_password(context: PasswordChangePageContext) -> Html<String> {
    Html(format!(
        "<h1>Change password</h1><p>{}</p><form method=\"post\" action=\"/change-password\"><input type=\"hidden\" name=\"_csrf\" value=\"{}\"><input type=\"password\" name=\"current_password\"><input type=\"password\" name=\"new_password\"><input type=\"password\" name=\"confirm_password\"><button>Change password</button></form>",
        auth_message(context.message),
        context.csrf_token.as_str()
    ))
}

/// Maps authentication state to the demo application's display text.
fn auth_message(message: Option<AuthMessage>) -> &'static str {
    match message {
        Some(AuthMessage::InvalidCredentials) => "Invalid credentials.",
        Some(AuthMessage::PasswordExpired) => "Change your password to continue.",
        Some(AuthMessage::PasswordMismatch) => "Passwords do not match.",
        Some(AuthMessage::PasswordLength) => "Use 12 to 128 characters.",
        Some(AuthMessage::PasswordUnchanged) => "Choose a different password.",
        Some(AuthMessage::PasswordChanged) => "Password changed; sign in again.",
        None => "",
    }
}

#[webstack::tokio::main(crate = "webstack::tokio")]
/// Composes and runs the demonstration application.
async fn main() -> anyhow::Result<()> {
    Application::builder()
        .auth_pages(get(login), get(change_password))?
        .route("/", get(index))?
        .authenticated_route("/account", get(|| async { "Authenticated" }))?
        .role_route("/admin", "admin", get(|| async { "Administrator" }))?
        .run()
        .await?;
    Ok(())
}
