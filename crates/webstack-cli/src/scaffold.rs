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
https_port = 8443
http_port = 8080
http_redirect = false

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
https_port = 8443
http_port = 8080
http_redirect = false

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
"#,
    },
    ScaffoldFile {
        path: "src/main.rs",
        contents: r#"use askama::Template;
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
    Application::builder()
        .assets::<Assets>()?
        .auth_pages(get(login_page), get(password_page))?
        .route("/", get(index))?
        .authenticated_route("/account", get(account))?
        .role_route("/admin", "admin", get(admin))?
        .run()
        .await?;
    Ok(())
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
    {{tailwind}} -i assets/css/input.css -o assets/css/app.css --minify

css-watch:
    {{tailwind}} -i assets/css/input.css -o assets/css/app.css --watch

run:
    bacon run

dev:
    just --parallel css-watch run

check:
    cargo fmt --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --all-features

release: css
    cargo build --release
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
",
    },
    ScaffoldFile {
        path: "docs/README.md",
        contents: r"# {{app_name}} Documentation

- [Architecture](architecture.md)
- [Development](development.md)
- [Deployment](deployment.md)
",
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
