use webstack::auth::{AuthMessage, AuthSession, LoginPageContext, PasswordChangePageContext};

use super::views::{LoginTemplate, PasswordTemplate};
use crate::web::navigation::navigation;

/// Renders the application-owned login page.
pub(crate) async fn login(context: LoginPageContext, auth: AuthSession) -> LoginTemplate {
    LoginTemplate {
        nav: navigation(&auth, &context.csrf_token),
        message: auth_message(context.message),
    }
}

/// Renders the application-owned password-change page.
pub(crate) async fn change_password(
    context: PasswordChangePageContext,
    auth: AuthSession,
) -> PasswordTemplate {
    PasswordTemplate {
        nav: navigation(&auth, &context.csrf_token),
        message: auth_message(context.message),
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
