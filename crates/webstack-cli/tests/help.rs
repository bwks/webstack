use std::process::Command;

#[test]
fn help_lists_the_initial_command_surface() {
    let output = Command::new(env!("CARGO_BIN_EXE_webstack"))
        .arg("--help")
        .output()
        .expect("webstack CLI should run");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");
    for command in ["new", "dev", "assets", "generate", "doctor"] {
        assert!(
            stdout.contains(command),
            "help output should include {command:?}: {stdout}"
        );
    }
}
