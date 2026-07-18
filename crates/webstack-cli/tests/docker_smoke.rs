use std::{
    net::{TcpListener, TcpStream},
    process::Command,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use tempfile::TempDir;

struct DockerGuard {
    container: String,
    image: String,
    volume: String,
}

impl DockerGuard {
    fn new(container: String, image: String, volume: String) -> Self {
        Self {
            container,
            image,
            volume,
        }
    }
}

impl Drop for DockerGuard {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "--force", &self.container])
            .output();
        let _ = Command::new("docker")
            .args(["volume", "rm", "--force", &self.volume])
            .output();
        let _ = Command::new("docker")
            .args(["image", "rm", "--force", &self.image])
            .output();
    }
}

fn assert_success(output: &std::process::Output, action: &str) {
    assert!(
        output.status.success(),
        "{action} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn available_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("temporary port")
        .local_addr()
        .expect("listener address")
        .port()
}

fn unique_name(prefix: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current timestamp")
        .as_nanos();
    format!("{prefix}-{}-{timestamp}", std::process::id())
}

fn wait_for_health(port: u16) {
    let deadline = Instant::now() + Duration::from_mins(1);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            let output = Command::new("curl")
                .args([
                    "--fail",
                    "--silent",
                    "--show-error",
                    &format!("http://127.0.0.1:{port}/healthz"),
                ])
                .output()
                .expect("health probe");
            if output.status.success() && output.stdout == br#"{"status":"ok"}"# {
                return;
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
    panic!("Alpine container did not become healthy");
}

#[test]
#[ignore = "downloads frontend tools and builds and runs the Alpine image"]
fn generated_alpine_image_runs_non_root_with_persistent_data() {
    let temp = TempDir::new().expect("temporary directory");
    let application = temp.path().join("alpine-smoke");
    let generated = Command::new(env!("CARGO_BIN_EXE_webstack"))
        .args(["new", "alpine-smoke"])
        .arg("--directory")
        .arg(&application)
        .output()
        .expect("application generation");
    assert_success(&generated, "application generation");

    let image = unique_name("webstack-alpine-smoke");
    let container = unique_name("webstack-alpine-container");
    let volume = unique_name("webstack-alpine-data");
    let _guard = DockerGuard::new(container.clone(), image.clone(), volume.clone());
    let build = Command::new("docker")
        .args(["build", "--tag", &image, "."])
        .current_dir(&application)
        .output()
        .expect("Docker build");
    assert_success(&build, "Alpine image build");

    let config_path = application.join("container.toml");
    let config = std::fs::read_to_string(application.join("webstack.toml"))
        .expect("generated configuration")
        .replace("bind_addr = \"127.0.0.1\"", "bind_addr = \"0.0.0.0\"");
    std::fs::write(&config_path, config).expect("container configuration");
    let port = available_port();
    let run = Command::new("docker")
        .args([
            "run",
            "--detach",
            "--name",
            &container,
            "--publish",
            &format!("127.0.0.1:{port}:8080"),
            "--mount",
            &format!(
                "type=bind,source={},target=/app/webstack.toml,readonly",
                config_path.display()
            ),
            "--mount",
            &format!("type=volume,source={volume},target=/app/data"),
            &image,
        ])
        .output()
        .expect("Docker run");
    assert_success(&run, "Alpine container start");
    wait_for_health(port);

    let user = Command::new("docker")
        .args(["inspect", "--format", "{{.Config.User}}", &container])
        .output()
        .expect("container inspection");
    assert_success(&user, "container user inspection");
    assert_eq!(String::from_utf8_lossy(&user.stdout).trim(), "10001:10001");

    let stop = Command::new("docker")
        .args(["stop", "--time", "35", &container])
        .output()
        .expect("container stop");
    assert_success(&stop, "graceful container stop");
}
