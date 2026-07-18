use std::{fs, path::Path, process::Command};

use syn::{
    Attribute, Expr, ForeignItem, ImplItem, Item, ItemImpl, Lit, Meta, Stmt, TraitItem, Type,
    visit::{self, Visit},
};
use tempfile::TempDir;

const APPLICATION_STRUCTURE: &[&str] = &[
    "src/main.rs",
    "src/application.rs",
    "src/domain/mod.rs",
    "src/domain/items/mod.rs",
    "src/domain/items/item.rs",
    "src/domain/items/queries.rs",
    "src/domain/items/commands.rs",
    "src/web/mod.rs",
    "src/web/router.rs",
    "src/web/assets.rs",
    "src/web/errors.rs",
    "src/web/home/mod.rs",
    "src/web/home/handlers.rs",
    "src/web/home/views.rs",
    "src/web/auth/mod.rs",
    "src/web/auth/handlers.rs",
    "src/web/auth/views.rs",
    "src/web/items/mod.rs",
    "src/web/items/handlers.rs",
    "src/web/items/views.rs",
    "templates/layouts/base.html.jinja",
    "templates/home/index.html.jinja",
    "templates/auth/login.html.jinja",
    "templates/auth/change_password.html.jinja",
    "templates/errors/page.html.jinja",
    "templates/errors/partial.html.jinja",
    "templates/items/index.html.jinja",
    "templates/items/region.html.jinja",
    "templates/items/edit.html.jinja",
    "templates/items/edit_form.html.jinja",
];

struct LocalTypeVisitor {
    found_local_type: bool,
}

impl<'ast> Visit<'ast> for LocalTypeVisitor {
    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        if matches!(statement, Stmt::Item(Item::Struct(_) | Item::Enum(_))) {
            self.found_local_type = true;
        }
        visit::visit_stmt(self, statement);
    }
}

fn webstack() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_webstack"));
    command.env("WEBSTACK_SKIP_ASSET_SETUP", "1");
    command
}

fn assert_justfile_dev_workflow(justfile: &str) {
    assert!(justfile.contains("css:"));
    assert!(justfile.contains("css-watch:"));
    assert!(justfile.contains("[parallel]\ndev: css-watch run"));
    assert!(!justfile.contains(concat!("just ", "--parallel")));
    assert!(justfile.contains("release: css"));
    assert!(justfile.contains("image: css"));
    assert!(justfile.contains("{{ tailwind }} -i assets/css/input.css"));
}

fn assert_favicon_links(template: &str) {
    assert!(template.contains(
        r#"href="/static/images/favicon-light.png" media="(prefers-color-scheme: light)""#
    ));
    assert!(template.contains(
        r#"href="/static/images/favicon-dark.png" media="(prefers-color-scheme: dark)""#
    ));
}

fn assert_generated_favicons(target: &Path) {
    let favicon_light =
        fs::read(target.join("assets/images/favicon-light.png")).expect("light favicon");
    let favicon_dark =
        fs::read(target.join("assets/images/favicon-dark.png")).expect("dark favicon");
    assert!(favicon_light.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(favicon_dark.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_ne!(favicon_light, favicon_dark);

    let base_template = fs::read_to_string(target.join("templates/layouts/base.html.jinja"))
        .expect("base template");
    let error_template = fs::read_to_string(target.join("templates/errors/page.html.jinja"))
        .expect("error template");
    assert_favicon_links(&base_template);
    assert_favicon_links(&error_template);
}

fn assert_generated_template_convention(target: &Path) {
    for relative in [
        "templates/base.html",
        "templates/base.html.jinja",
        "templates/pages/index.html",
        "templates/pages/index.html.jinja",
        "templates/pages/login.html",
        "templates/pages/login.html.jinja",
        "templates/pages/change_password.html",
        "templates/pages/change_password.html.jinja",
        "templates/pages/error.html",
        "templates/pages/error.html.jinja",
        "templates/pages/items.html",
        "templates/pages/items.html.jinja",
        "templates/pages/item_edit.html",
        "templates/pages/item_edit.html.jinja",
        "templates/partials/error.html",
        "templates/partials/error.html.jinja",
        "templates/partials/items_region.html",
        "templates/partials/items_region.html.jinja",
        "templates/partials/item_edit.html",
        "templates/partials/item_edit.html.jinja",
    ] {
        assert!(
            !target.join(relative).exists(),
            "legacy template {relative}"
        );
    }

    let annotations = [
        "src/web/errors.rs",
        "src/web/home/views.rs",
        "src/web/auth/views.rs",
        "src/web/items/views.rs",
    ]
    .map(|relative| fs::read_to_string(target.join(relative)).expect("generated Rust source"))
    .join("\n");
    assert_eq!(
        annotations
            .matches(r#".html.jinja", ext = "html")]"#)
            .count(),
        9
    );
    assert!(!annotations.contains(r#".html")]"#));

    let conventions =
        fs::read_to_string(target.join("docs/CONVENTIONS.md")).expect("generated conventions");
    assert!(conventions.contains("`errors/page.html.jinja`"));
    assert!(conventions.contains("`errors/partial.html.jinja`"));
    assert!(!conventions.contains("`pages/error.html`"));
    assert!(!conventions.contains("`partials/error.html`"));
}

fn assert_in_order(output: &str, expected: &[&str]) {
    let mut remaining = output;
    for value in expected {
        let position = remaining.find(value).unwrap_or_else(|| {
            panic!("output does not contain {value:?} after prior values:\n{output}")
        });
        remaining = &remaining[position + value.len()..];
    }
}

fn assert_generation_progress(output: &str, target: &Path) {
    assert_in_order(
        output,
        &[
            &format!("Creating {}...", target.display()),
            "  Writing application scaffold... done",
            &format!("Created {}", target.display()),
            "git init -b main",
            "just dev",
        ],
    );
    assert!(!output.contains("Downloading and verifying frontend assets"));
    assert!(!output.contains("Installing frontend assets"));
}

fn assert_generated_application_files(target: &Path) {
    assert_application_structure(target);
    for relative in [
        "Cargo.toml",
        "Dockerfile",
        ".dockerignore",
        ".github/workflows/ci.yml",
        "deploy/inventory-app.service",
        "webstack.toml",
        "webstack.example.toml",
        "assets/css/input.css",
        "assets/js/htmx.min.js",
        "assets/images/.gitkeep",
        "assets/images/favicon-light.png",
        "assets/images/favicon-dark.png",
        "migrations/0001_initialize.surql",
        "docs/README.md",
        "docs/CONVENTIONS.md",
        "tests/application.rs",
    ] {
        assert!(target.join(relative).is_file(), "missing {relative}");
    }
}

fn assert_application_structure(target: &Path) {
    for relative in APPLICATION_STRUCTURE {
        assert!(target.join(relative).is_file(), "missing {relative}");
    }
}

fn impl_target(item: &ItemImpl) -> Option<String> {
    let Type::Path(target) = item.self_ty.as_ref() else {
        return None;
    };
    target
        .path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

fn assert_type_and_impl_order(items: &[Item], path: &Path) {
    let mut adjacent_type = None;
    let mut free_function_seen = false;
    for item in items {
        match item {
            Item::Struct(item) => {
                assert!(
                    !free_function_seen,
                    "{} declares struct {} after a free function",
                    path.display(),
                    item.ident
                );
                adjacent_type = Some(item.ident.to_string());
            }
            Item::Enum(item) => {
                assert!(
                    !free_function_seen,
                    "{} declares enum {} after a free function",
                    path.display(),
                    item.ident
                );
                adjacent_type = Some(item.ident.to_string());
            }
            Item::Impl(item) => {
                let target = impl_target(item)
                    .unwrap_or_else(|| panic!("{} has an unsupported impl target", path.display()));
                assert_eq!(
                    adjacent_type.as_deref(),
                    Some(target.as_str()),
                    "{} separates the impl for {target} from its type declaration",
                    path.display()
                );
                adjacent_type = Some(target);
            }
            Item::Fn(_) => {
                adjacent_type = None;
                free_function_seen = true;
            }
            Item::Mod(item) => {
                if let Some((_, items)) = &item.content {
                    assert_type_and_impl_order(items, path);
                }
                adjacent_type = None;
            }
            _ => adjacent_type = None,
        }
    }
}

fn has_doc_comment(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        if !attribute.path().is_ident("doc") {
            return false;
        }
        let Meta::NameValue(meta) = &attribute.meta else {
            return false;
        };
        let Expr::Lit(expression) = &meta.value else {
            return false;
        };
        let Lit::Str(value) = &expression.lit else {
            return false;
        };
        !value.value().trim().is_empty()
    })
}

fn is_test_module(item: &syn::ItemMod) -> bool {
    item.attrs.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && matches!(&attribute.meta, Meta::List(meta) if meta.tokens.to_string() == "test")
    })
}

fn assert_documented_functions(items: &[Item], path: &Path) {
    for item in items {
        match item {
            Item::Fn(item) => assert!(
                has_doc_comment(&item.attrs),
                "{} has undocumented function {}",
                path.display(),
                item.sig.ident
            ),
            Item::Impl(item) => {
                for implementation_item in &item.items {
                    if let ImplItem::Fn(function) = implementation_item {
                        assert!(
                            has_doc_comment(&function.attrs),
                            "{} has undocumented method {}",
                            path.display(),
                            function.sig.ident
                        );
                    }
                }
            }
            Item::Trait(item) => {
                for trait_item in &item.items {
                    if let TraitItem::Fn(function) = trait_item {
                        assert!(
                            has_doc_comment(&function.attrs),
                            "{} has undocumented trait method {}",
                            path.display(),
                            function.sig.ident
                        );
                    }
                }
            }
            Item::ForeignMod(item) => {
                for foreign_item in &item.items {
                    if let ForeignItem::Fn(function) = foreign_item {
                        assert!(
                            has_doc_comment(&function.attrs),
                            "{} has undocumented foreign function {}",
                            path.display(),
                            function.sig.ident
                        );
                    }
                }
            }
            Item::Mod(item) if !is_test_module(item) => {
                if let Some((_, items)) = &item.content {
                    assert_documented_functions(items, path);
                }
            }
            _ => {}
        }
    }
}

fn assert_documented_rust_file(path: &Path) {
    let source = fs::read_to_string(path).expect("Rust source");
    let syntax = syn::parse_file(&source).expect("valid Rust source");
    assert_documented_functions(&syntax.items, path);
}

fn assert_generated_rust_policy(root: &Path) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            pending.extend(
                fs::read_dir(&path)
                    .expect("Rust source directory")
                    .map(|entry| entry.expect("Rust source entry").path()),
            );
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let source = fs::read_to_string(&path).expect("Rust source");
            let syntax = syn::parse_file(&source).expect("valid Rust source");
            assert_documented_functions(&syntax.items, &path);
            assert_type_and_impl_order(&syntax.items, &path);

            let mut visitor = LocalTypeVisitor {
                found_local_type: false,
            };
            visitor.visit_file(&syntax);
            assert!(
                !visitor.found_local_type,
                "{} declares a struct or enum inside a block",
                path.display()
            );
        }
    }
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn help_lists_the_initial_command_surface() {
    let output = webstack().arg("--help").output().expect("CLI should run");
    assert_success(&output);

    let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");
    for command in ["new", "assets", "generate", "doctor"] {
        assert!(stdout.contains(command), "help should include {command:?}");
    }
    assert!(
        !stdout.contains("\n  dev "),
        "help should not advertise dev"
    );
}

#[test]
fn version_and_unknown_commands_are_handled_by_clap() {
    let version = webstack()
        .arg("--version")
        .output()
        .expect("CLI should run");
    assert_success(&version);
    assert!(String::from_utf8_lossy(&version.stdout).contains(env!("CARGO_PKG_VERSION")));

    let unknown = webstack().arg("unknown").output().expect("CLI should run");
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unrecognized subcommand"));
}

#[test]
fn new_generates_an_application_owned_project() {
    let temp = TempDir::new().expect("temporary directory");
    let target = temp.path().join("inventory-app");
    let output = webstack()
        .args(["new", "inventory-app"])
        .arg("--directory")
        .arg(&target)
        .output()
        .expect("CLI should run");
    assert_success(&output);

    assert_generated_application_files(&target);
    assert_generated_template_convention(&target);

    let manifest = fs::read_to_string(target.join("Cargo.toml")).expect("manifest");
    assert!(manifest.contains("name = \"inventory-app\""));
    assert!(manifest.contains("branch = \"framework-baseline\""));
    assert!(manifest.contains("\n[workspace]\n"));
    assert!(manifest.contains("anyhow = \"1\""));
    assert!(manifest.contains("askama = \"0.16\""));
    assert!(manifest.contains("[profile.release]"));
    assert!(manifest.contains("lto = true"));
    let main = fs::read_to_string(target.join("src/main.rs")).expect("main source");
    assert!(main.contains("async fn main() -> anyhow::Result<()>"));
    assert!(main.contains("application::run().await"));
    assert!(!main.contains("Application::builder()"));
    let application =
        fs::read_to_string(target.join("src/application.rs")).expect("application source");
    assert!(application.contains("web::router::build()?.run().await?"));
    let router = fs::read_to_string(target.join("src/web/router.rs")).expect("router source");
    assert!(router.contains("Application::builder()"));
    assert!(!router.contains("ApplicationSettings"));
    assert!(router.contains(".assets::<Assets>()?"));
    assert!(router.contains(".error_renderer(errors::render)?"));
    assert!(router.contains(".auth_pages(get(auth::login_page), get(auth::password_page))?"));
    assert!(router.contains(".authenticated_route(\"/account\", get(home::account))?"));
    assert!(router.contains(".role_route(\"/admin\", \"admin\", get(home::admin))?"));
    assert!(!main.contains("struct Migrations"));
    assert!(!main.contains(".migrations::<"));
    assert!(router.contains(".route(\"/\", get(home::index))?"));
    assert!(!main.contains("webstack::observability::init"));
    assert!(!target.join("src/errors.rs").exists());
    assert!(!target.join("src/items.rs").exists());

    let item_context =
        fs::read_to_string(target.join("src/domain/items/mod.rs")).expect("item context");
    for function in ["list", "get", "create", "update", "delete"] {
        assert!(
            item_context.contains(&format!("pub(crate) async fn {function}")),
            "missing Items context function {function}"
        );
    }
    let item_queries =
        fs::read_to_string(target.join("src/domain/items/queries.rs")).expect("item queries");
    let item_commands =
        fs::read_to_string(target.join("src/domain/items/commands.rs")).expect("item commands");
    let item_handlers =
        fs::read_to_string(target.join("src/web/items/handlers.rs")).expect("item handlers");
    assert!(item_queries.contains("SELECT id, name"));
    assert!(!item_queries.contains("retry_write"));
    assert!(item_commands.contains("retry_write"));
    assert!(item_commands.contains("CREATE item"));
    assert!(item_commands.contains("UPDATE ONLY"));
    assert!(item_commands.contains("DELETE ONLY"));
    assert!(!item_handlers.contains("retry_write"));
    assert!(!item_handlers.contains(".query("));
    assert!(item_handlers.contains("Form(form): Form<ItemForm>"));
    assert!(item_handlers.contains("items::create(&state, &form.name).await?"));
    let local_config = fs::read_to_string(target.join("webstack.toml")).expect("local config");
    let example_config =
        fs::read_to_string(target.join("webstack.example.toml")).expect("example config");
    assert!(local_config.contains("environment = \"development\""));
    assert!(example_config.contains("environment = \"production\""));
    assert_ne!(local_config, example_config);
    assert!(local_config.contains("htmx_version = \"4.0.0-beta5\""));
    assert!(local_config.contains("password_ttl_days = 90"));
    assert!(local_config.contains("shutdown_timeout_seconds = 30"));
    let migration = fs::read_to_string(target.join("migrations/0001_initialize.surql"))
        .expect("initial migration");
    assert!(migration.contains("DEFINE TABLE _webstack_user SCHEMAFULL"));
    assert!(migration.contains("DEFINE TABLE _webstack_session SCHEMAFULL"));
    assert!(migration.contains("DEFINE TABLE item SCHEMAFULL"));
    assert!(!target.join("Cargo.lock").exists());
    assert!(!target.join(".git").exists());
    assert!(!target.join("assets/css/app.css").exists());
    assert_generated_favicons(&target);
    let justfile = fs::read_to_string(target.join("justfile")).expect("justfile");
    assert_justfile_dev_workflow(&justfile);
    let dockerfile = fs::read_to_string(target.join("Dockerfile")).expect("Dockerfile");
    assert!(dockerfile.contains("FROM rust:1.97.0-alpine3.24 AS builder"));
    assert!(dockerfile.contains("FROM alpine:3.24"));
    assert!(dockerfile.contains("USER 10001:10001"));
    assert!(dockerfile.contains("COPY --from=builder /build/migrations /app/migrations"));
    assert!(!dockerfile.contains("debian"));
    let dockerignore = fs::read_to_string(target.join(".dockerignore")).expect(".dockerignore");
    assert!(dockerignore.contains("webstack.toml"));
    assert!(!dockerignore.contains("assets/css/app.css"));
    let service =
        fs::read_to_string(target.join("deploy/inventory-app.service")).expect("systemd unit");
    assert!(service.contains("TimeoutStopSec=35s"));
    assert!(service.contains("User=inventory-app"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_generation_progress(&stdout, &target);
}

#[test]
fn generated_application_nested_in_framework_is_its_own_workspace() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let parent = tempfile::Builder::new()
        .prefix("nested-generated-app-")
        .tempdir_in(workspace)
        .expect("temporary directory inside framework workspace");
    let target = parent.path().join("nested-app");
    let generated = webstack()
        .args(["new", "nested-app"])
        .arg("--directory")
        .arg(&target)
        .arg("--framework-path")
        .arg(workspace)
        .output()
        .expect("CLI should run");
    assert_success(&generated);

    let metadata = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .env("CARGO_NET_OFFLINE", "true")
        .current_dir(target)
        .output()
        .expect("cargo metadata should run");
    assert_success(&metadata);
}

#[test]
fn example_application_matches_the_generated_structure() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let demo = workspace.join("examples/demo");
    assert_application_structure(&demo);

    for relative in [
        "src/web/navigation.rs",
        "templates/account/index.html.jinja",
        "templates/admin/index.html.jinja",
    ] {
        assert!(demo.join(relative).is_file(), "missing {relative}");
    }
    for relative in [
        "src/errors.rs",
        "src/items.rs",
        "templates/base.html.jinja",
        "templates/pages",
        "templates/partials",
    ] {
        assert!(!demo.join(relative).exists(), "legacy path {relative}");
    }

    let queries =
        fs::read_to_string(demo.join("src/domain/items/queries.rs")).expect("demo item queries");
    let commands =
        fs::read_to_string(demo.join("src/domain/items/commands.rs")).expect("demo item commands");
    let handlers =
        fs::read_to_string(demo.join("src/web/items/handlers.rs")).expect("demo item handlers");
    assert!(queries.contains("SELECT id, name"));
    assert!(!queries.contains("retry_write"));
    assert!(commands.contains("retry_write"));
    assert!(commands.contains("CREATE item"));
    assert!(commands.contains("UPDATE ONLY"));
    assert!(commands.contains("DELETE ONLY"));
    assert!(!handlers.contains("retry_write"));
    assert!(!handlers.contains(".query("));
}

#[test]
fn generate_migration_numbers_valid_history_and_prints_the_path() {
    let temp = TempDir::new().expect("temporary directory");
    let migrations = temp.path().join("migrations");
    fs::create_dir(&migrations).expect("migrations directory");
    let initial = webstack()
        .args(["generate", "migration", "initialize"])
        .current_dir(temp.path())
        .output()
        .expect("CLI should run");
    assert_success(&initial);
    assert!(migrations.join("0001_initialize.surql").is_file());
    fs::write(migrations.join("0004_add_items.surql"), "-- Items\n").expect("migration");

    let output = webstack()
        .args(["generate", "migration", "add_roles"])
        .current_dir(temp.path())
        .output()
        .expect("CLI should run");
    assert_success(&output);
    let target = migrations.join("0005_add_roles.surql");
    assert!(target.is_file());
    assert_eq!(
        fs::read_to_string(&target).expect("generated migration"),
        "-- Migration: add_roles\n\n"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(&target.display().to_string()));
}

#[test]
fn generate_migration_rejects_invalid_inputs_and_history() {
    let temp = TempDir::new().expect("temporary directory");
    let migrations = temp.path().join("migrations");
    fs::create_dir(&migrations).expect("migrations directory");

    let invalid_name = webstack()
        .args(["generate", "migration", "Bad-name"])
        .current_dir(temp.path())
        .output()
        .expect("CLI should run");
    assert!(!invalid_name.status.success());
    assert!(String::from_utf8_lossy(&invalid_name.stderr).contains("snake_case"));

    fs::write(migrations.join("bad.surql"), "-- malformed\n").expect("migration");
    let malformed = webstack()
        .args(["generate", "migration", "add_roles"])
        .current_dir(temp.path())
        .output()
        .expect("CLI should run");
    assert!(!malformed.status.success());
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("invalid existing migration"));
}

#[test]
fn generate_migration_rejects_name_collisions_and_overflow() {
    let temp = TempDir::new().expect("temporary directory");
    let migrations = temp.path().join("migrations");
    fs::create_dir(&migrations).expect("migrations directory");
    fs::write(migrations.join("0007_add_roles.surql"), "-- Existing\n").expect("migration");

    let collision = webstack()
        .args(["generate", "migration", "add_roles"])
        .current_dir(temp.path())
        .output()
        .expect("CLI should run");
    assert!(!collision.status.success());
    assert!(String::from_utf8_lossy(&collision.stderr).contains("already exists"));

    fs::remove_file(migrations.join("0007_add_roles.surql")).expect("remove migration");
    fs::write(migrations.join("9999_final.surql"), "-- Final\n").expect("migration");
    let overflow = webstack()
        .args(["generate", "migration", "another"])
        .current_dir(temp.path())
        .output()
        .expect("CLI should run");
    assert!(!overflow.status.success());
    assert!(String::from_utf8_lossy(&overflow.stderr).contains("overflow"));
}

#[test]
fn generator_diagnostics_require_verbose_mode() {
    let temp = TempDir::new().expect("temporary directory");
    let quiet_target = temp.path().join("quiet-app");
    let quiet = webstack()
        .args(["new", "quiet-app"])
        .arg("--directory")
        .arg(&quiet_target)
        .output()
        .expect("CLI should run");
    assert_success(&quiet);
    assert!(quiet.stderr.is_empty(), "quiet stderr should be empty");

    let verbose_target = temp.path().join("verbose-app");
    let verbose = webstack()
        .args(["-v", "new", "verbose-app"])
        .arg("--directory")
        .arg(&verbose_target)
        .output()
        .expect("CLI should run");
    assert_success(&verbose);
    let stderr = String::from_utf8_lossy(&verbose.stderr);
    assert!(stderr.contains("application scaffold written"), "{stderr}");
    assert!(stderr.contains("app.name=verbose-app"), "{stderr}");
}

#[test]
fn assets_setup_reports_download_progress_before_an_error() {
    let temp = TempDir::new().expect("temporary directory");
    fs::write(temp.path().join("webstack.toml"), "not valid toml =")
        .expect("invalid configuration");
    let output = webstack()
        .args(["assets", "setup"])
        .current_dir(temp.path())
        .output()
        .expect("CLI should run");

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Downloading and verifying frontend assets...\n"
    );
}

#[test]
fn new_rejects_invalid_names_before_creating_the_target() {
    let temp = TempDir::new().expect("temporary directory");
    let target = temp.path().join("Bad_Name");
    let output = webstack()
        .args(["new", "Bad_Name"])
        .arg("--directory")
        .arg(&target)
        .output()
        .expect("CLI should run");

    assert!(!output.status.success());
    assert!(!target.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("kebab-case"));
}

#[test]
fn new_refuses_an_existing_target_without_modifying_it() {
    let temp = TempDir::new().expect("temporary directory");
    let target = temp.path().join("inventory-app");
    fs::create_dir(&target).expect("existing target");
    fs::write(target.join("keep.txt"), "unchanged").expect("sentinel");

    let output = webstack()
        .args(["new", "inventory-app"])
        .arg("--directory")
        .arg(&target)
        .output()
        .expect("CLI should run");

    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(target.join("keep.txt")).expect("sentinel"),
        "unchanged"
    );
    assert!(!target.join("Cargo.toml").exists());
}

#[test]
fn new_leaves_version_control_and_dependency_resolution_to_the_developer() {
    let temp = TempDir::new().expect("temporary directory");
    let target = temp.path().join("inventory-app");
    let output = webstack()
        .args(["new", "inventory-app"])
        .arg("--directory")
        .arg(&target)
        .output()
        .expect("CLI should run");
    assert_success(&output);

    assert!(!target.join(".git").exists());
    assert!(!target.join("Cargo.lock").exists());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("git init -b main"));
    assert!(stdout.contains("just dev"));
}

#[test]
fn generated_project_uses_only_the_webstack_facade() {
    let temp = TempDir::new().expect("temporary directory");
    let target = temp.path().join("inventory-app");
    let framework = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let output = webstack()
        .args(["new", "inventory-app"])
        .arg("--directory")
        .arg(&target)
        .arg("--framework-path")
        .arg(framework)
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .expect("CLI should run");
    assert_success(&output);

    let check = Command::new("cargo")
        .args(["check", "--all-targets"])
        .env("CARGO_NET_OFFLINE", "true")
        .current_dir(&target)
        .output()
        .expect("cargo check should run");
    assert_success(&check);
    assert!(target.join("Cargo.lock").is_file());

    let manifest = fs::read_to_string(target.join("Cargo.toml")).expect("manifest");
    assert!(manifest.contains("webstack = { path ="));
    assert!(!manifest.contains("webstack-core"));
}

#[test]
fn production_framework_sources_do_not_launch_external_processes() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let mut pending = vec![workspace.join("crates"), workspace.join("examples")];

    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).expect("framework source directory") {
            let path = entry.expect("source entry").path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "tests") {
                    continue;
                }
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let source = fs::read_to_string(&path).expect("Rust source");
                for forbidden in [
                    "std::process::Command",
                    "process::Command",
                    "process::{Command",
                    "Command::new(",
                    "tokio::process",
                    "duct::",
                    "subprocess::",
                    "xshell::",
                ] {
                    assert!(
                        !source.contains(forbidden),
                        "{} contains forbidden process launching API {forbidden:?}",
                        path.display()
                    );
                }
            }
        }
    }
}

#[test]
fn types_are_module_scoped_and_impls_are_adjacent() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let mut pending = vec![workspace.join("crates"), workspace.join("examples")];

    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).expect("Rust source directory") {
            let path = entry.expect("source entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let source = fs::read_to_string(&path).expect("Rust source");
                let syntax = syn::parse_file(&source).expect("valid Rust source");
                assert_type_and_impl_order(&syntax.items, &path);

                let mut visitor = LocalTypeVisitor {
                    found_local_type: false,
                };
                visitor.visit_file(&syntax);
                assert!(
                    !visitor.found_local_type,
                    "{} declares a struct or enum inside a block",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn production_and_generated_functions_are_documented() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let mut pending = vec![workspace.join("crates"), workspace.join("examples")];

    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).expect("Rust source directory") {
            let path = entry.expect("source entry").path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "tests") {
                    continue;
                }
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                assert_documented_rust_file(&path);
            }
        }
    }

    let temp = TempDir::new().expect("temporary directory");
    let target = temp.path().join("documented-app");
    let output = webstack()
        .args(["new", "documented-app"])
        .arg("--directory")
        .arg(&target)
        .output()
        .expect("CLI should run");
    assert_success(&output);
    assert_generated_rust_policy(&target.join("src"));
    assert_documented_rust_file(&target.join("build.rs"));
}

#[test]
#[should_panic(expected = "undocumented function undocumented")]
fn documentation_policy_rejects_an_undocumented_function() {
    let syntax = syn::parse_file("fn undocumented() {}").expect("valid Rust source");
    assert_documented_functions(&syntax.items, Path::new("undocumented.rs"));
}
