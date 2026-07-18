use webstack::auth::{AuthSession, CsrfToken};

use super::views::{AccountTemplate, AdminTemplate, IndexTemplate};
use crate::web::navigation::navigation;

/// Renders the demonstration application's public home page.
pub(crate) async fn index(auth: AuthSession, csrf: CsrfToken) -> IndexTemplate {
    IndexTemplate {
        nav: navigation(&auth, &csrf),
    }
}

/// Renders the authenticated account summary.
pub(crate) async fn account(auth: AuthSession, csrf: CsrfToken) -> AccountTemplate {
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
pub(crate) async fn admin(auth: AuthSession, csrf: CsrfToken) -> AdminTemplate {
    AdminTemplate {
        nav: navigation(&auth, &csrf),
    }
}
