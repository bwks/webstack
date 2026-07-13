use std::{fs, path::Path, process::Command};

use tempfile::TempDir;

fn webstack() -> Command {
    Command::new(env!("CARGO_BIN_EXE_webstack"))
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

    for relative in [
        "Cargo.toml",
        "src/main.rs",
        "webstack.toml",
        "webstack.example.toml",
        "assets/css/input.css",
        "assets/js/htmx.min.js",
        "assets/images/.gitkeep",
        "templates/.gitkeep",
        "migrations/.gitkeep",
        "docs/README.md",
        "tests/application.rs",
    ] {
        assert!(target.join(relative).is_file(), "missing {relative}");
    }

    let manifest = fs::read_to_string(target.join("Cargo.toml")).expect("manifest");
    assert!(manifest.contains("name = \"inventory-app\""));
    assert!(manifest.contains("branch = \"framework-baseline\""));
    assert!(manifest.contains("anyhow = \"1\""));
    let main = fs::read_to_string(target.join("src/main.rs")).expect("main source");
    assert!(main.contains("async fn main() -> anyhow::Result<()>"));
    assert!(main.contains("Application::builder()"));
    assert!(!main.contains("ApplicationSettings"));
    assert!(main.contains(".route(\"/\", get(index))?"));
    assert!(main.contains(".run()"));
    assert!(!main.contains("webstack::observability::init"));
    assert_eq!(
        fs::read_to_string(target.join("webstack.toml")).expect("local config"),
        fs::read_to_string(target.join("webstack.example.toml")).expect("example config")
    );
    assert!(!target.join("Cargo.lock").exists());
    assert!(!target.join(".git").exists());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("git init -b main"));
    assert!(stdout.contains("cargo check"));
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
    assert!(stdout.contains("cargo check"));
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
    let mut pending = vec![workspace.join("crates")];

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
                    "tokio::process",
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
