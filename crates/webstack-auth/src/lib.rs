#![doc = "Authentication, sessions, authorization, and CSRF support."]

mod backend;
mod csrf;
mod error;
mod routes;
mod session_store;
mod throttle;
mod user;

pub use backend::{AuthBackend, Credentials, bootstrap_admin, hash_password};
pub use csrf::{CsrfToken, csrf_middleware};
pub use error::AuthError;
pub use routes::{
    AuthMessage, AuthRuntime, LoginPageContext, PasswordChangePageContext, RequiredRole,
    auth_router, require_authenticated, require_role,
};
pub use session_store::TursoSessionStore;
pub use throttle::LoginThrottle;
pub use user::User;

pub type AuthSession = axum_login::AuthSession<AuthBackend>;

pub const ADMIN_ROLE: &str = "admin";
pub const USER_ROLE: &str = "user";
