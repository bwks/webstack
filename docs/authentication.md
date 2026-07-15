# Authentication and Authorisation

Webstack provides local accounts through `axum-login`, Argon2id password hashes,
and a `tower-sessions` store backed by the application's one shared SurrealDB
instance. It does not provide public self-registration yet. Account management
can be added later behind the same backend boundary without changing session or
route APIs.

## First Start

When `auth.bootstrap_admin = true` and no users exist, startup creates username
`admin` with temporary password `changeme` and roles `admin` and `user`. The
credential is documented but never logged.

The root `environment` setting controls its expiry:

- `development` sets `password_expires_at` to `3000-12-31T00:00:00Z` for a
  convenient local first run.
- `production` sets it to the initialization time, forcing a password change
  immediately after the first login.

Generated ignored `webstack.toml` files use development. Committed
`webstack.example.toml` files use production. Never deploy the development
setting or retain `changeme` on a reachable service.

Passwords must contain 12 through 128 characters. A successful change stores a
fresh Argon2id hash, sets expiry to `password_ttl_days` from the change time
(90 days by default), invalidates sessions carrying the old authentication
hash, and requires signing in again.

## Application-Owned Pages and Protected Routes

Applications register their own GET pages while Webstack owns the form actions:

```rust
Application::builder()
    .auth_pages(get(login_page), get(password_page))?
    .authenticated_route("/account", get(account))?
    .role_route("/admin", "admin", get(admin))?;
```

Login page handlers extract `LoginPageContext`; password page handlers extract
`PasswordChangePageContext`. Both provide a CSRF token and an optional safe
`AuthMessage` for application-owned copy. Role names must begin with a lowercase
letter and otherwise contain lowercase ASCII letters or underscores. `admin`
and `user` are conventional built-ins; applications may define additional
roles with the same naming contract.

Anonymous normal requests receive a redirect to `/login`. htmx requests receive
`401` with `HX-Redirect: /login`, so a login page is not swapped into a partial.
Authenticated users with expired passwords are sent to `/change-password`.

## Sessions, CSRF, and Throttling

Sessions have a seven-day default absolute lifetime and are checked on protected
routes. Expired database rows are removed daily. Cookies are HTTP-only and use
SameSite protection; TLS cookie hardening is completed with the in-process TLS
phase.

Every application route rejects unsafe methods unless the session token,
double-submit cookie, and `X-CSRF-Token` header match. Generated base templates
put the header in `hx-headers`. Framework login, logout, and password forms use
the same token in a hidden `_csrf` field.

Failed logins are keyed by client IP and normalized username. The fifth
consecutive failure starts a one-second delay, which doubles to a maximum of 60
seconds; inactive entries expire after 15 minutes. Responses never distinguish
an unknown username, wrong password, or disabled account.
