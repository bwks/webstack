pub(crate) struct ScaffoldFile {
    pub(crate) path: &'static str,
    pub(crate) contents: &'static str,
}

const CONFIG: &str = r#"environment = "development"

[assets]
tailwind_version = "4.3.1"
daisyui_version = "5.6.18"
htmx_version = "4.0.0-beta5"

[server]
bind_addr = "127.0.0.1"
# Disabled TLS serves HTTP on http_port. TLS modes serve HTTPS on https_port.
https_port = 8443
http_port = 8080
# In a TLS mode, this also binds http_port and redirects it to HTTPS.
http_redirect = false
shutdown_timeout_seconds = 30

[tls]
mode = "disabled"
domain = ""
acme_email = ""
acme_cache_dir = "./data/acme"
acme_staging = false

[database]
data_dir = "./data/surreal"
namespace = "app"
database = "app"

[auth]
session_ttl_hours = 168
bootstrap_admin = true
password_ttl_days = 90

[backup]
enabled = false
cron = "0 0 3 * * *"
retention = 14

[backup.r2]
account_id = ""
bucket = ""
prefix = "db-backups/"
# Credentials are accepted only from R2_ACCESS_KEY_ID and R2_SECRET_ACCESS_KEY.

[observability]
filter = "info"
format = "pretty"
"#;

const EXAMPLE_CONFIG: &str = r#"environment = "production"

[assets]
tailwind_version = "4.3.1"
daisyui_version = "5.6.18"
htmx_version = "4.0.0-beta5"

[server]
bind_addr = "127.0.0.1"
# Disabled TLS serves HTTP on http_port. TLS modes serve HTTPS on https_port.
https_port = 8443
http_port = 8080
# In a TLS mode, this also binds http_port and redirects it to HTTPS.
http_redirect = false
shutdown_timeout_seconds = 30

[tls]
mode = "disabled"
domain = ""
acme_email = ""
acme_cache_dir = "./data/acme"
acme_staging = false

[database]
data_dir = "./data/surreal"
namespace = "app"
database = "app"

[auth]
session_ttl_hours = 168
bootstrap_admin = true
password_ttl_days = 90

[backup]
enabled = false
cron = "0 0 3 * * *"
retention = 14

[backup.r2]
account_id = ""
bucket = ""
prefix = "db-backups/"
# Credentials are accepted only from R2_ACCESS_KEY_ID and R2_SECRET_ACCESS_KEY.

[observability]
filter = "info"
format = "pretty"
"#;

pub(crate) const FILES: &[ScaffoldFile] = &[
    ScaffoldFile {
        path: "Cargo.toml",
        contents: r#"[package]
name = "{{app_name}}"
version = "0.1.0"
edition = "2024"
rust-version = "1.97"
publish = false

[dependencies]
anyhow = "1"
askama = "0.16"
askama_web = { version = "0.16", features = ["axum-0.8", "tracing-0.1"] }
rust-embed = "8"
webstack = {{framework_dependency}}

[lints.rust]
unsafe_code = "forbid"

[profile.release]
codegen-units = 1
lto = true
strip = true
"#,
    },
    ScaffoldFile {
        path: "src/main.rs",
        contents: r#"mod errors;
mod items;

use askama::Template;
use askama_web::WebTemplate;
use webstack::axum::routing::get;
use webstack::auth::AuthMessage;
use webstack::prelude::*;

#[derive(rust_embed::RustEmbed)]
#[folder = "assets/"]
struct Assets;

#[derive(Template, WebTemplate)]
#[template(path = "pages/index.html")]
struct IndexTemplate {
    app_name: &'static str,
    csrf_token: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "pages/login.html")]
struct LoginTemplate {
    csrf_token: String,
    message: &'static str,
}

#[derive(Template, WebTemplate)]
#[template(path = "pages/change_password.html")]
struct PasswordTemplate {
    csrf_token: String,
    message: &'static str,
}

/// Renders the generated application's home page.
async fn index(csrf: CsrfToken) -> IndexTemplate {
    IndexTemplate {
        app_name: "{{app_name}}",
        csrf_token: csrf.as_str().to_owned(),
    }
}

/// Renders the application-owned login page.
async fn login_page(context: LoginPageContext) -> LoginTemplate {
    LoginTemplate {
        csrf_token: context.csrf_token.as_str().to_owned(),
        message: auth_message(context.message),
    }
}

/// Renders the application-owned mandatory password-change page.
async fn password_page(context: PasswordChangePageContext) -> PasswordTemplate {
    PasswordTemplate {
        csrf_token: context.csrf_token.as_str().to_owned(),
        message: auth_message(context.message),
    }
}

/// Renders a minimal authenticated account endpoint.
async fn account() -> &'static str {
    "Authenticated account"
}

/// Renders a minimal administrator-only endpoint.
async fn admin() -> &'static str {
    "Administrator access"
}

/// Maps framework-safe authentication state to application-owned copy.
fn auth_message(message: Option<AuthMessage>) -> &'static str {
    match message {
        Some(AuthMessage::InvalidCredentials) => "The username or password was not accepted.",
        Some(AuthMessage::PasswordExpired) => "Change your password to continue.",
        Some(AuthMessage::PasswordMismatch) => "The new passwords do not match.",
        Some(AuthMessage::PasswordLength) => "Passwords must contain 12 to 128 characters.",
        Some(AuthMessage::PasswordUnchanged) => "Choose a password different from the current password.",
        Some(AuthMessage::PasswordChanged) => "Password changed. Sign in again.",
        None => "",
    }
}

#[webstack::tokio::main(crate = "webstack::tokio")]
/// Composes and runs the generated Webstack application.
async fn main() -> anyhow::Result<()> {
    let application = Application::builder()
        .assets::<Assets>()?
        .error_renderer(errors::render)?
        .auth_pages(get(login_page), get(password_page))?
        .route("/", get(index))?
        .authenticated_route("/account", get(account))?
        .role_route("/admin", "admin", get(admin))?;
    items::routes(application)?.run().await?;
    Ok(())
}
"#,
    },
    ScaffoldFile {
        path: "src/errors.rs",
        contents: r#"use askama::Template;
use webstack::ErrorView;

#[derive(Template)]
#[template(path = "pages/error.html")]
struct ErrorPageTemplate<'a> {
    error: &'a ErrorView,
}

#[derive(Template)]
#[template(path = "partials/error.html")]
struct ErrorPartialTemplate<'a> {
    error: &'a ErrorView,
}

/// Renders safe application errors with application-owned templates.
pub(crate) fn render(error: &ErrorView) -> Option<String> {
    if error.is_htmx() {
        ErrorPartialTemplate { error }.render().ok()
    } else {
        ErrorPageTemplate { error }.render().ok()
    }
}
"#,
    },
    ScaffoldFile {
        path: "src/items.rs",
        contents: r#"use askama::Template;
use askama_web::WebTemplate;
use webstack::{
    AppError, AppState, ApplicationBuilder,
    auth::CsrfToken,
    axum::{
        Form,
        extract::{Path, State},
        response::{IntoResponse, Redirect, Response},
        routing::{get, post, put},
    },
    database::retry_write,
    htmx::HxRequest,
    serde::Deserialize,
    surrealdb::types::{RecordId, RecordIdKey, SurrealValue},
};

const ITEM_ROLE: &str = "user";

#[derive(Clone, Debug)]
struct ItemView {
    id: String,
    name: String,
}

#[derive(Debug, SurrealValue)]
#[surreal(crate = "webstack::surrealdb::types")]
struct ItemRecord {
    id: RecordId,
    name: String,
}

impl ItemRecord {
    /// Converts a database record into display-safe application data.
    fn into_view(self) -> Result<ItemView, AppError> {
        let RecordIdKey::String(id) = self.id.key else {
            return Err(AppError::internal(
                "read item identifier",
                std::io::Error::other("item identifier was not a string"),
            ));
        };
        Ok(ItemView {
            id,
            name: self.name,
        })
    }
}

#[derive(Deserialize)]
#[serde(crate = "webstack::serde")]
struct ItemForm {
    name: String,
}

impl ItemForm {
    /// Returns a normalized item name or a safe validation error.
    fn validated_name(&self) -> Result<String, AppError> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(AppError::validation(
                "Item names must contain between 1 and 100 characters.",
            ));
        }
        Ok(name.to_owned())
    }
}

#[derive(Template, WebTemplate)]
#[template(path = "pages/items.html")]
struct ItemsPageTemplate {
    items: Vec<ItemView>,
    csrf_token: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "partials/items_region.html")]
struct ItemsRegionTemplate {
    items: Vec<ItemView>,
    csrf_token: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "pages/item_edit.html")]
struct ItemEditPageTemplate {
    item: ItemView,
    csrf_token: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "partials/item_edit.html")]
struct ItemEditPartialTemplate {
    item: ItemView,
    csrf_token: String,
}

/// Adds the reference Items feature to an application builder.
pub(crate) fn routes(application: ApplicationBuilder) -> Result<ApplicationBuilder, webstack::ApplicationError> {
    application
        .authenticated_route("/items", get(index))?
        .role_route("/items", ITEM_ROLE, post(create))?
        .authenticated_route("/items/{id}/edit", get(edit))?
        .role_route("/items/{id}", ITEM_ROLE, put(update).delete(remove).post(update))?
        .role_route("/items/{id}/delete", ITEM_ROLE, post(remove))
}

/// Renders either the full Items page or its stable htmx region.
async fn index(
    State(state): State<AppState>,
    htmx: HxRequest,
    csrf: CsrfToken,
) -> Result<Response, AppError> {
    render_items(&state, htmx, &csrf).await
}

/// Creates an item and then renders the post-mutation representation.
async fn create(
    State(state): State<AppState>,
    htmx: HxRequest,
    csrf: CsrfToken,
    Form(form): Form<ItemForm>,
) -> Result<Response, AppError> {
    let name = form.validated_name()?;
    let database = state.database().clone();
    retry_write(|| {
        let database = database.clone();
        let name = name.clone();
        async move {
            database
                .query("CREATE item SET name = $name;")
                .bind(("name", name))
                .await?
                .check()?;
            Ok(())
        }
    })
    .await
    .map_err(|source| AppError::internal("create item", source))?;
    mutation_response(&state, htmx, &csrf).await
}

/// Renders an item's full-page or partial edit form.
async fn edit(
    State(state): State<AppState>,
    Path(id): Path<String>,
    htmx: HxRequest,
    csrf: CsrfToken,
) -> Result<Response, AppError> {
    let item = load_item(&state, &id).await?;
    let csrf_token = csrf.as_str().to_owned();
    if htmx.is_htmx() {
        Ok(ItemEditPartialTemplate { item, csrf_token }.into_response())
    } else {
        Ok(ItemEditPageTemplate { item, csrf_token }.into_response())
    }
}

/// Updates an item and then renders the post-mutation representation.
async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    htmx: HxRequest,
    csrf: CsrfToken,
    Form(form): Form<ItemForm>,
) -> Result<Response, AppError> {
    let name = form.validated_name()?;
    let database = state.database().clone();
    let found = retry_write(|| {
        let database = database.clone();
        let id = id.clone();
        let name = name.clone();
        async move {
            let mut response = database
                .query("UPDATE ONLY type::record('item', $id) SET name = $name RETURN id, name;")
                .bind(("id", id))
                .bind(("name", name))
                .await?
                .check()?;
            response.take::<Option<ItemRecord>>(0)
        }
    })
    .await
    .map_err(|source| AppError::internal("update item", source))?;
    if found.is_none() {
        return Err(AppError::not_found("The requested item does not exist."));
    }
    mutation_response(&state, htmx, &csrf).await
}

/// Deletes an item and then renders the post-mutation representation.
async fn remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
    htmx: HxRequest,
    csrf: CsrfToken,
) -> Result<Response, AppError> {
    let database = state.database().clone();
    let found = retry_write(|| {
        let database = database.clone();
        let id = id.clone();
        async move {
            let mut response = database
                .query("DELETE ONLY type::record('item', $id) RETURN BEFORE;")
                .bind(("id", id))
                .await?
                .check()?;
            response.take::<Option<ItemRecord>>(0)
        }
    })
    .await
    .map_err(|source| AppError::internal("delete item", source))?;
    if found.is_none() {
        return Err(AppError::not_found("The requested item does not exist."));
    }
    mutation_response(&state, htmx, &csrf).await
}

/// Returns the refreshed region to htmx or a redirect to ordinary browsers.
async fn mutation_response(
    state: &AppState,
    htmx: HxRequest,
    csrf: &CsrfToken,
) -> Result<Response, AppError> {
    if htmx.is_htmx() {
        render_items(state, htmx, csrf).await
    } else {
        Ok(Redirect::to("/items").into_response())
    }
}

/// Loads and renders the current collection representation.
async fn render_items(
    state: &AppState,
    htmx: HxRequest,
    csrf: &CsrfToken,
) -> Result<Response, AppError> {
    let items = load_items(state).await?;
    let csrf_token = csrf.as_str().to_owned();
    if htmx.is_htmx() {
        Ok(ItemsRegionTemplate { items, csrf_token }.into_response())
    } else {
        Ok(ItemsPageTemplate { items, csrf_token }.into_response())
    }
}

/// Loads all shared items in stable creation order.
async fn load_items(state: &AppState) -> Result<Vec<ItemView>, AppError> {
    let mut response = state
        .database()
        .query("SELECT id, name, created_at FROM item ORDER BY created_at, id;")
        .await
        .map_err(|source| AppError::internal("load items", source))?
        .check()
        .map_err(|source| AppError::internal("load items", source))?;
    response
        .take::<Vec<ItemRecord>>(0)
        .map_err(|source| AppError::internal("decode items", source))?
        .into_iter()
        .map(ItemRecord::into_view)
        .collect()
}

/// Loads one item by its string record identifier.
async fn load_item(state: &AppState, id: &str) -> Result<ItemView, AppError> {
    let mut response = state
        .database()
        .query("SELECT id, name FROM ONLY type::record('item', $id);")
        .bind(("id", id.to_owned()))
        .await
        .map_err(|source| AppError::internal("load item", source))?
        .check()
        .map_err(|source| AppError::internal("load item", source))?;
    response
        .take::<Option<ItemRecord>>(0)
        .map_err(|source| AppError::internal("decode item", source))?
        .ok_or_else(|| AppError::not_found("The requested item does not exist."))?
        .into_view()
}
"#,
    },
    ScaffoldFile {
        path: "build.rs",
        contents: r#"/// Verifies release assets and configures Cargo rebuild inputs.
fn main() {
    println!("cargo::rerun-if-changed=assets");
    println!("cargo::rerun-if-changed=templates");
    let release = std::env::var("PROFILE").is_ok_and(|profile| profile == "release");
    if release && !std::path::Path::new("assets/css/app.css").is_file() {
        panic!("assets/css/app.css is missing; run `just css` before a release build");
    }
}
"#,
    },
    ScaffoldFile {
        path: "rust-toolchain.toml",
        contents: r#"[toolchain]
channel = "stable"
components = ["clippy", "rustfmt"]
profile = "default"
"#,
    },
    ScaffoldFile {
        path: ".gitignore",
        contents: r"/target/
/assets/css/app.css
/tools/tailwindcss
/tools/tailwindcss.exe
/tools/daisyui.js
/tools/daisyui-theme.js
/data/
/webstack.toml
",
    },
    ScaffoldFile {
        path: ".dockerignore",
        contents: r".git/
.github/
target/
tools/
data/
webstack.toml
Dockerfile
",
    },
    ScaffoldFile {
        path: "Dockerfile",
        contents: r#"FROM rust:1.97.0-alpine3.24 AS builder

RUN apk add --no-cache \
    build-base \
    ca-certificates \
    clang \
    clang-dev \
    cmake \
    git \
    linux-headers \
    perl

ENV CC=clang \
    CXX=clang++ \
    LIBCLANG_PATH=/usr/lib

WORKDIR /build
COPY . .
RUN cargo build --release

FROM alpine:3.24

RUN apk add --no-cache ca-certificates libgcc libstdc++ \
    && addgroup -g 10001 -S webstack \
    && adduser -u 10001 -S -D -H -G webstack webstack \
    && mkdir -p /app/data \
    && chown -R webstack:webstack /app

WORKDIR /app
COPY --from=builder /build/target/release/{{app_name}} /app/{{app_name}}
COPY --from=builder /build/migrations /app/migrations

USER 10001:10001
VOLUME ["/app/data"]
EXPOSE 8080 8443
ENTRYPOINT ["/app/{{app_name}}"]
"#,
    },
    ScaffoldFile {
        path: "deploy/{{app_name}}.service",
        contents: r"[Unit]
Description={{app_name}} Webstack application
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User={{app_name}}
Group={{app_name}}
WorkingDirectory=/opt/{{app_name}}
ExecStart=/opt/{{app_name}}/{{app_name}}
Restart=on-failure
RestartSec=5s
TimeoutStopSec=35s
NoNewPrivileges=true
PrivateTmp=true
ProtectHome=true
ProtectSystem=strict
ReadWritePaths=/var/lib/{{app_name}}

[Install]
WantedBy=multi-user.target
",
    },
    ScaffoldFile {
        path: ".github/workflows/ci.yml",
        contents: r"name: CI

on:
  pull_request:
  push:
    branches: [main]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy,rustfmt
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --check
      - run: cargo clippy --all-targets --all-features -- -D warnings
      - run: cargo test --all-features

  image:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo install --git https://github.com/bwks/webstack.git --branch framework-baseline webstack-cli
      - run: just setup
      - run: just image
",
    },
    ScaffoldFile {
        path: "README.md",
        contents: r"# {{app_name}}

A Webstack application. See [`docs/README.md`](docs/README.md).

## First Run

```sh
git init -b main
cargo check
just dev
```

Commit the `Cargo.lock` created by the first Cargo command.

The development bootstrap login is `admin` / `changeme`. Change it before
exposing the application outside a trusted local environment.
",
    },
    ScaffoldFile {
        path: "webstack.toml",
        contents: CONFIG,
    },
    ScaffoldFile {
        path: "webstack.example.toml",
        contents: EXAMPLE_CONFIG,
    },
    ScaffoldFile {
        path: "justfile",
        contents: r#"tailwind := if os() == "windows" { "tools/tailwindcss.exe" } else { "tools/tailwindcss" }

setup:
    webstack assets setup

css:
    {{ tailwind }} -i assets/css/input.css -o assets/css/app.css --minify

css-watch:
    {{ tailwind }} -i assets/css/input.css -o assets/css/app.css --watch

run:
    bacon run

[parallel]
dev: css-watch run

check:
    cargo fmt --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --all-features

release: css
    cargo build --release

image: css
    docker build --tag {{app_name}}:local .
"#,
    },
    ScaffoldFile {
        path: "bacon.toml",
        contents: r#"[jobs.run]
command = ["cargo", "run"]
need_stdout = true

[jobs.check]
command = ["cargo", "check", "--all-targets", "--all-features"]
need_stdout = false

[jobs.clippy]
command = ["cargo", "clippy", "--all-targets", "--all-features", "--", "-D", "warnings"]
need_stdout = false

[jobs.test]
command = ["cargo", "test", "--all-features"]
need_stdout = true
"#,
    },
    ScaffoldFile {
        path: "assets/css/input.css",
        contents: r#"@import "tailwindcss";
@source "../../src";
@source "../../templates";
@plugin "../../tools/daisyui.js";
"#,
    },
    ScaffoldFile {
        path: "assets/js/htmx.min.js",
        contents: r"/* Replaced with the configured verified htmx release by `webstack new`. */
",
    },
    ScaffoldFile {
        path: "assets/images/.gitkeep",
        contents: "",
    },
    ScaffoldFile {
        path: "templates/base.html",
        contents: r#"<!doctype html>
<html lang="en" data-theme="light">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{% block title %}Webstack{% endblock %}</title>
    <link rel="stylesheet" href="/static/css/app.css">
    <script src="/static/js/htmx.min.js" defer></script>
  </head>
  <body class="min-h-screen bg-base-200 text-base-content" hx-headers='{"X-CSRF-Token":"{{ csrf_token }}"}'>
    <main class="mx-auto flex min-h-screen max-w-5xl items-center px-6 py-16">
      {% block content %}{% endblock %}
    </main>
  </body>
</html>
"#,
    },
    ScaffoldFile {
        path: "templates/pages/index.html",
        contents: r#"{% extends "base.html" %}

{% block title %}{{ app_name }} · Webstack{% endblock %}

{% block content %}
<section class="hero rounded-box bg-base-100 shadow-xl">
  <div class="hero-content py-16 text-center">
    <div class="max-w-2xl">
      <div class="badge badge-primary badge-outline mb-5">Webstack</div>
      <h1 class="text-5xl font-bold tracking-tight">{{ app_name }}</h1>
      <p class="py-6 text-lg opacity-75">
        Axum, Askama, htmx, Tailwind CSS, and daisyUI are wired together and ready for your application.
      </p>
      <a class="btn btn-primary" href="/healthz">Check application health</a>
    </div>
  </div>
</section>
{% endblock %}
"#,
    },
    ScaffoldFile {
        path: "templates/pages/login.html",
        contents: r#"{% extends "base.html" %}

{% block title %}Sign in · Webstack{% endblock %}

{% block content %}
<section class="card mx-auto w-full max-w-md bg-base-100 shadow-xl">
  <form class="card-body" method="post" action="/login">
    <h1 class="card-title text-3xl">Sign in</h1>
    {% if !message.is_empty() %}<div class="alert alert-warning">{{ message }}</div>{% endif %}
    <input type="hidden" name="_csrf" value="{{ csrf_token }}">
    <label class="form-control"><span class="label-text">Username</span><input class="input input-bordered" name="username" autocomplete="username" required></label>
    <label class="form-control"><span class="label-text">Password</span><input class="input input-bordered" type="password" name="password" autocomplete="current-password" required></label>
    <button class="btn btn-primary mt-4" type="submit">Sign in</button>
  </form>
</section>
{% endblock %}
"#,
    },
    ScaffoldFile {
        path: "templates/pages/change_password.html",
        contents: r#"{% extends "base.html" %}

{% block title %}Change password · Webstack{% endblock %}

{% block content %}
<section class="card mx-auto w-full max-w-md bg-base-100 shadow-xl">
  <form class="card-body" method="post" action="/change-password">
    <h1 class="card-title text-3xl">Change password</h1>
    {% if !message.is_empty() %}<div class="alert alert-warning">{{ message }}</div>{% endif %}
    <input type="hidden" name="_csrf" value="{{ csrf_token }}">
    <label class="form-control"><span class="label-text">Current password</span><input class="input input-bordered" type="password" name="current_password" autocomplete="current-password" required></label>
    <label class="form-control"><span class="label-text">New password</span><input class="input input-bordered" type="password" name="new_password" autocomplete="new-password" minlength="12" maxlength="128" required></label>
    <label class="form-control"><span class="label-text">Confirm password</span><input class="input input-bordered" type="password" name="confirm_password" autocomplete="new-password" minlength="12" maxlength="128" required></label>
    <button class="btn btn-primary mt-4" type="submit">Change password</button>
  </form>
</section>
{% endblock %}
"#,
    },
    ScaffoldFile {
        path: "templates/pages/error.html",
        contents: r#"<!doctype html>
<html lang="en" data-theme="light">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{{ error.title() }} · Webstack</title>
    <link rel="stylesheet" href="/static/css/app.css">
  </head>
  <body class="min-h-screen bg-base-200 text-base-content">
    <main class="mx-auto flex min-h-screen max-w-2xl items-center px-6 py-16">
      <section class="card w-full bg-base-100 shadow-xl"><div class="card-body">
        <p class="text-sm font-semibold opacity-60">{{ error.status().as_u16() }}</p>
        <h1 class="card-title text-3xl">{{ error.title() }}</h1>
        <p>{{ error.message() }}</p>
        <div class="card-actions"><a class="btn btn-primary" href="/">Return home</a></div>
      </div></section>
    </main>
  </body>
</html>
"#,
    },
    ScaffoldFile {
        path: "templates/partials/error.html",
        contents: r#"<div class="alert alert-error" role="alert">
  <strong>{{ error.title() }}</strong>
  <span>{{ error.message() }}</span>
</div>
"#,
    },
    ScaffoldFile {
        path: "templates/pages/items.html",
        contents: r#"{% extends "base.html" %}

{% block title %}Items · Webstack{% endblock %}

{% block content %}
<section class="w-full space-y-6">
  <div><a class="link" href="/">Home</a><h1 class="text-4xl font-bold">Items</h1><p class="opacity-70">A shared reference CRUD feature.</p></div>
  {% include "partials/items_region.html" %}
</section>
{% endblock %}
"#,
    },
    ScaffoldFile {
        path: "templates/partials/items_region.html",
        contents: r##"<section id="items-region" class="space-y-5">
  <div id="item-errors" aria-live="polite"></div>
  <form class="card bg-base-100 shadow" method="post" action="/items"
        hx-post="/items" hx-target="#items-region" hx-swap="outerHTML"
        hx-headers='{"X-CSRF-Token":"{{ csrf_token }}"}'
        hx-status:4xx="target:#item-errors" hx-status:5xx="target:#item-errors">
    <div class="card-body sm:flex-row sm:items-end">
      <input type="hidden" name="_csrf" value="{{ csrf_token }}">
      <label class="form-control grow"><span class="label-text">Name</span><input class="input input-bordered w-full" name="name" minlength="1" maxlength="100" required></label>
      <button class="btn btn-primary" type="submit">Add item</button>
    </div>
  </form>
  <div class="space-y-3">
  {% for item in items %}
    <article class="card bg-base-100 shadow"><div class="card-body flex-row items-center justify-between">
      <span>{{ item.name }}</span>
      <div class="flex gap-2">
        <a class="btn btn-sm" href="/items/{{ item.id }}/edit" hx-get="/items/{{ item.id }}/edit" hx-target="#items-region" hx-swap="outerHTML">Edit</a>
        <form method="post" action="/items/{{ item.id }}/delete" hx-delete="/items/{{ item.id }}" hx-target="#items-region" hx-swap="outerHTML" hx-headers='{"X-CSRF-Token":"{{ csrf_token }}"}' hx-status:4xx="target:#item-errors" hx-status:5xx="target:#item-errors">
          <input type="hidden" name="_csrf" value="{{ csrf_token }}"><button class="btn btn-error btn-sm" type="submit">Delete</button>
        </form>
      </div>
    </div></article>
  {% else %}
    <p class="rounded-box bg-base-100 p-6 text-center opacity-70">No items yet.</p>
  {% endfor %}
  </div>
</section>
"##,
    },
    ScaffoldFile {
        path: "templates/pages/item_edit.html",
        contents: r#"{% extends "base.html" %}

{% block title %}Edit item · Webstack{% endblock %}

{% block content %}
<section class="w-full space-y-6"><h1 class="text-4xl font-bold">Edit item</h1>{% include "partials/item_edit.html" %}</section>
{% endblock %}
"#,
    },
    ScaffoldFile {
        path: "templates/partials/item_edit.html",
        contents: r##"<section id="items-region" class="space-y-4">
  <div id="item-errors" aria-live="polite"></div>
  <form class="card bg-base-100 shadow" method="post" action="/items/{{ item.id }}"
        hx-put="/items/{{ item.id }}" hx-target="#items-region" hx-swap="outerHTML"
        hx-headers='{"X-CSRF-Token":"{{ csrf_token }}"}'
        hx-status:4xx="target:#item-errors" hx-status:5xx="target:#item-errors">
    <div class="card-body">
      <input type="hidden" name="_csrf" value="{{ csrf_token }}">
      <label class="form-control"><span class="label-text">Name</span><input class="input input-bordered" name="name" value="{{ item.name }}" minlength="1" maxlength="100" required></label>
      <div class="card-actions"><button class="btn btn-primary" type="submit">Save</button><a class="btn" href="/items" hx-get="/items" hx-target="#items-region" hx-swap="outerHTML">Cancel</a></div>
    </div>
  </form>
</section>
"##,
    },
    ScaffoldFile {
        path: "migrations/0001_initialize.surql",
        contents: r"-- Initial application and authentication schema.
DEFINE TABLE _webstack_user SCHEMAFULL;
DEFINE FIELD username ON _webstack_user TYPE string;
DEFINE FIELD password_hash ON _webstack_user TYPE string;
DEFINE FIELD roles ON _webstack_user TYPE array<string>;
DEFINE FIELD disabled ON _webstack_user TYPE bool;
DEFINE FIELD created_at ON _webstack_user TYPE int;
DEFINE FIELD password_expires_at ON _webstack_user TYPE int;
DEFINE INDEX webstack_user_username ON _webstack_user FIELDS username UNIQUE;

DEFINE TABLE _webstack_session SCHEMAFULL;
DEFINE FIELD payload ON _webstack_session TYPE string;
DEFINE FIELD expires_at ON _webstack_session TYPE int;
DEFINE INDEX webstack_session_expiry ON _webstack_session FIELDS expires_at;

DEFINE TABLE item SCHEMAFULL;
DEFINE FIELD name ON item TYPE string ASSERT string::len($value) >= 1 AND string::len($value) <= 100;
DEFINE FIELD created_at ON item TYPE datetime DEFAULT time::now();
DEFINE FIELD updated_at ON item TYPE datetime DEFAULT time::now() VALUE time::now();
",
    },
    ScaffoldFile {
        path: "docs/README.md",
        contents: r"# {{app_name}} Documentation

- [Architecture](architecture.md)
- [Development](development.md)
- [Deployment](deployment.md)
- [Application conventions](CONVENTIONS.md)
",
    },
    ScaffoldFile {
        path: "docs/CONVENTIONS.md",
        contents: r#"# Application Conventions

## Full pages and htmx partials

Handlers extract `HxRequest` and render a full page for ordinary navigation or a partial for htmx. Every replaceable partial owns one stable outer element; the Items example uses `#items-region` with `hx-swap="outerHTML"`.

## Forms and CSRF

Unsafe forms include a hidden `_csrf` field for ordinary browser submission and an explicit `X-CSRF-Token` `hx-headers` value for htmx. Keep both: htmx 4 does not implicitly inherit attributes. Validation errors target a stable `#item-errors` live region.

## Errors

Return `AppError` from application handlers. Public variants contain safe display text. Wrap database and other internal failures with `AppError::internal`; Webstack logs the source and renders only generic copy. The application error renderer selects `pages/error.html` for ordinary requests and `partials/error.html` for htmx.

## Database writes

Use `retry_write` only around transaction-safe database operations. Never perform email, network calls, logging with business meaning, or other external side effects inside its closure because the closure can run four times. Render templates and perform follow-up reads after the retried operation.
"#,
    },
    ScaffoldFile {
        path: "docs/architecture.md",
        contents: r"# Architecture

This application uses the Webstack facade and owns its domain code, templates, assets, and migrations.

Webstack owns the local account backend, SurrealDB session store, CSRF checks, and route guards. The application owns authentication page templates and display copy.
",
    },
    ScaffoldFile {
        path: "docs/development.md",
        contents: r#"# Development

Run `just dev` for the server and Tailwind watcher. Run `just check` before committing.

Frontend versions are controlled by the Webstack-owned `[assets]` section of `webstack.toml`. Run `just setup` after changing a version.

Webstack loads `webstack.toml`, applies runtime migrations, bootstraps the development administrator when needed, and initializes tracing before serving. Use `webstack::tracing` for structured application events and never log secrets.

The local configuration uses `environment = "development"`, where the documented `admin` / `changeme` bootstrap credential remains usable for local setup. Password changes require 12 to 128 characters.
"#,
    },
    ScaffoldFile {
        path: "docs/deployment.md",
        contents: r#"# Deployment

Run `just release`. The release binary embeds templates, CSS, JavaScript, and application assets. Deploy the complete `migrations/` directory beside it.

Start from `webstack.example.toml`, which uses `environment = "production"`. A first-run `admin` / `changeme` login is forced immediately to `/change-password`; replace it before serving application routes.

## Alpine container

Run `just setup` once, then `just image`. The multi-stage image builds on Alpine 3.24 and runs as UID/GID 10001 with only CA certificates and the C++ runtime installed. Mount a production configuration at `/app/webstack.toml` and persistent storage at `/app/data`. Set `server.bind_addr = "0.0.0.0"` and `database.data_dir = "./data/surreal"` in the mounted configuration.

The image contains the complete migration history. Probe `/healthz` from the orchestrator; no diagnostic client is added to the runtime image.

## systemd

Install the release binary, `webstack.toml`, and `migrations/` under `/opt/{{app_name}}`. Create the unprivileged `{{app_name}}` account, create `/var/lib/{{app_name}}`, and adjust `database.data_dir` and `tls.acme_cache_dir` to use that writable directory. Install `deploy/{{app_name}}.service`, reload systemd, and enable the service.

The service allows 35 seconds for Webstack's default 30-second graceful shutdown. Ports 8080 and 8443 need no capabilities; production firewalls may map 443 to 8443 without terminating TLS. Inspect structured logs with `journalctl -u {{app_name}}`.
"#,
    },
    ScaffoldFile {
        path: "tests/application.rs",
        contents: r"use webstack::Application;

#[test]
fn application_can_be_composed_from_the_facade() {
    let _application = Application::builder();
}
",
    },
];
