use webstack::auth::{AuthSession, CsrfToken};

#[derive(Clone)]
pub(crate) struct Navigation {
    pub(crate) authenticated: bool,
    pub(crate) username: String,
    pub(crate) is_admin: bool,
    pub(crate) csrf_token: String,
}

/// Builds navigation display state from the current session and CSRF token.
pub(crate) fn navigation(auth: &AuthSession, csrf: &CsrfToken) -> Navigation {
    let user = auth.user.as_ref();
    Navigation {
        authenticated: user.is_some(),
        username: user.map_or_else(String::new, |user| user.username().to_owned()),
        is_admin: user.is_some_and(|user| user.has_role("admin")),
        csrf_token: csrf.as_str().to_owned(),
    }
}
