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
    for command in ["new", "dev", "assets", "generate", "doctor"] {
        assert!(stdout.contains(command), "help should include {command:?}");
    }
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
        .args(["new", "inventory-app", "--no-git", "--no-lock"])
        .arg("--directory")
        .arg(&target)
        .output()
        .expect("CLI should run");
    assert_success(&output);

    for relative in [
        "Cargo.toml",
        "src/main.rs",
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
    assert!(main.contains("fn main() -> anyhow::Result<()>"));
    assert!(main.contains("webstack::observability::init"));
    assert!(!target.join("Cargo.lock").exists());
    assert!(!target.join(".git").exists());
}

#[test]
fn generator_diagnostics_require_verbose_mode() {
    let temp = TempDir::new().expect("temporary directory");
    let quiet_target = temp.path().join("quiet-app");
    let quiet = webstack()
        .args(["new", "quiet-app", "--no-git", "--no-lock"])
        .arg("--directory")
        .arg(&quiet_target)
        .output()
        .expect("CLI should run");
    assert_success(&quiet);
    assert!(quiet.stderr.is_empty(), "quiet stderr should be empty");

    let verbose_target = temp.path().join("verbose-app");
    let verbose = webstack()
        .args(["-v", "new", "verbose-app", "--no-git", "--no-lock"])
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
        .args(["new", "Bad_Name", "--no-git", "--no-lock"])
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
        .args(["new", "inventory-app", "--no-git", "--no-lock"])
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
fn new_initializes_a_main_git_branch_by_default() {
    let temp = TempDir::new().expect("temporary directory");
    let target = temp.path().join("inventory-app");
    let output = webstack()
        .args(["new", "inventory-app", "--no-lock"])
        .arg("--directory")
        .arg(&target)
        .output()
        .expect("CLI should run");
    assert_success(&output);

    assert!(target.join(".git").is_dir());
    let branch = Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(&target)
        .output()
        .expect("git should run");
    assert_success(&branch);
    assert_eq!(String::from_utf8_lossy(&branch.stdout).trim(), "main");
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
        .args(["new", "inventory-app", "--no-git"])
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
