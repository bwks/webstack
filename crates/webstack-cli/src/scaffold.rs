pub(crate) struct ScaffoldFile {
    pub(crate) path: &'static str,
    pub(crate) contents: &'static str,
}

const CONFIG: &str = r#"[assets]
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
use webstack::prelude::*;

#[derive(rust_embed::RustEmbed)]
#[folder = "assets/"]
struct Assets;

#[derive(Template, WebTemplate)]
#[template(path = "pages/index.html")]
struct IndexTemplate {
    app_name: &'static str,
}

async fn index() -> IndexTemplate {
    IndexTemplate {
        app_name: "{{app_name}}",
    }
}

#[webstack::tokio::main(crate = "webstack::tokio")]
async fn main() -> anyhow::Result<()> {
    Application::builder()
        .assets::<Assets>()?
        .route("/", get(index))?
        .run()
        .await?;
    Ok(())
}
"#,
    },
    ScaffoldFile {
        path: "build.rs",
        contents: r#"fn main() {
    println!("cargo::rerun-if-changed=assets");
    println!("cargo::rerun-if-changed=templates");
    println!("cargo::rerun-if-changed=migrations");

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
",
    },
    ScaffoldFile {
        path: "webstack.toml",
        contents: CONFIG,
    },
    ScaffoldFile {
        path: "webstack.example.toml",
        contents: CONFIG,
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
  <body class="min-h-screen bg-base-200 text-base-content">
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
        path: "migrations/.gitkeep",
        contents: "",
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
",
    },
    ScaffoldFile {
        path: "docs/development.md",
        contents: r"# Development

Run `just dev` for the server and Tailwind watcher. Run `just check` before committing.

Frontend versions are controlled by the Webstack-owned `[assets]` section of `webstack.toml`. Run `just setup` after changing a version.

Webstack loads `webstack.toml` and initializes tracing before serving. Use `webstack::tracing` for structured application events and never log secrets.
",
    },
    ScaffoldFile {
        path: "docs/deployment.md",
        contents: r"# Deployment

Run `just release`. The release binary embeds templates, CSS, JavaScript, and application assets.
",
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
