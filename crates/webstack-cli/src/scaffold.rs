pub(crate) struct ScaffoldFile {
    pub(crate) path: &'static str,
    pub(crate) contents: &'static str,
}

const CONFIG: &str = r#"[server]
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
webstack = {{framework_dependency}}

[lints.rust]
unsafe_code = "forbid"
"#,
    },
    ScaffoldFile {
        path: "src/main.rs",
        contents: r#"use webstack::prelude::*;
use webstack::axum::routing::get;

async fn index() -> &'static str {
    "{{app_name}}"
}

#[webstack::tokio::main(crate = "webstack::tokio")]
async fn main() -> anyhow::Result<()> {
    Application::builder()
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
        contents: r"check:
    cargo fmt --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --all-features
",
    },
    ScaffoldFile {
        path: "bacon.toml",
        contents: r#"[jobs.check]
command = ["cargo", "check", "--all-targets", "--all-features"]
need_stdout = false
"#,
    },
    ScaffoldFile {
        path: "assets/css/input.css",
        contents: r"/* Tailwind and daisyUI configuration is added by the assets milestone. */
",
    },
    ScaffoldFile {
        path: "assets/js/htmx.min.js",
        contents: r"/* Vendored htmx is added by the assets milestone. */
",
    },
    ScaffoldFile {
        path: "assets/images/.gitkeep",
        contents: "",
    },
    ScaffoldFile {
        path: "templates/.gitkeep",
        contents: "",
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

Run `cargo test` and `cargo clippy --all-targets --all-features -- -D warnings` before committing.

Webstack loads `webstack.toml` and initializes tracing before serving. Use `webstack::tracing` for structured application events and never log secrets.
",
    },
    ScaffoldFile {
        path: "docs/deployment.md",
        contents: r"# Deployment

Deployment behavior will be documented as runtime support is implemented.
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
