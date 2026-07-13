pub(crate) struct ScaffoldFile {
    pub(crate) path: &'static str,
    pub(crate) contents: &'static str,
}

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

fn main() -> anyhow::Result<()> {
    webstack::observability::init(&ObservabilityConfig::default())?;
    let _application = Application::new();
    webstack::tracing::info!(application = "{{app_name}}", "application composed");
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
        contents: "/target/\n/assets/css/app.css\n/tools/tailwindcss\n/data/\n/config.toml\n",
    },
    ScaffoldFile {
        path: "README.md",
        contents: "# {{app_name}}\n\nA Webstack application. See [`docs/README.md`](docs/README.md).\n",
    },
    ScaffoldFile {
        path: "config.example.toml",
        contents: "# Application configuration will be added with the runtime milestone.\n",
    },
    ScaffoldFile {
        path: "justfile",
        contents: "check:\n    cargo fmt --check\n    cargo clippy --all-targets --all-features -- -D warnings\n    cargo test --all-features\n",
    },
    ScaffoldFile {
        path: "bacon.toml",
        contents: "[jobs.check]\ncommand = [\"cargo\", \"check\", \"--all-targets\", \"--all-features\"]\nneed_stdout = false\n",
    },
    ScaffoldFile {
        path: "assets/css/input.css",
        contents: "/* Tailwind and daisyUI configuration is added by the assets milestone. */\n",
    },
    ScaffoldFile {
        path: "assets/js/htmx.min.js",
        contents: "/* Vendored htmx is added by the assets milestone. */\n",
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
        contents: "# {{app_name}} Documentation\n\n- [Architecture](architecture.md)\n- [Development](development.md)\n- [Deployment](deployment.md)\n",
    },
    ScaffoldFile {
        path: "docs/architecture.md",
        contents: "# Architecture\n\nThis application uses the Webstack facade and owns its domain code, templates, assets, and migrations.\n",
    },
    ScaffoldFile {
        path: "docs/development.md",
        contents: "# Development\n\nRun `cargo test` and `cargo clippy --all-targets --all-features -- -D warnings` before committing.\n\nThe binary initializes Webstack tracing at info level. Use `webstack::tracing` for structured application events and never log secrets.\n",
    },
    ScaffoldFile {
        path: "docs/deployment.md",
        contents: "# Deployment\n\nDeployment behavior will be documented as runtime support is implemented.\n",
    },
    ScaffoldFile {
        path: "tests/application.rs",
        contents: r"use webstack::Application;

#[test]
fn application_can_be_composed_from_the_facade() {
    let _application = Application::new();
}
",
    },
];
