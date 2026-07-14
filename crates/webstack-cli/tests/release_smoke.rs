use std::{
    fs,
    io::{Read as _, Write as _},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use tempfile::TempDir;

struct ChildGuard {
    child: Child,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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
        .expect("temporary port should bind")
        .local_addr()
        .expect("temporary listener should have an address")
        .port()
}

fn executable(path: &Path, name: &str) -> PathBuf {
    path.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

fn request(port: u16, path: &str) -> std::io::Result<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

fn wait_for_server(port: u16, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if request(port, "/healthz").is_ok() {
            return;
        }
        if let Some(status) = child.try_wait().expect("application status") {
            panic!("release application exited before serving: {status}");
        }
        assert!(
            Instant::now() < deadline,
            "release application did not start within 30 seconds"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

fn assert_ok_response(response: &str, expected: &str) {
    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "unexpected response status: {response}"
    );
    assert!(
        response.contains(expected),
        "response does not contain {expected:?}"
    );
}

#[test]
#[ignore = "downloads frontend tools and performs a generated application release build"]
fn generated_release_runs_with_binary_config_and_migrations() {
    let temp = TempDir::new().expect("temporary directory");
    let application = temp.path().join("phase-one-smoke");
    let deployment = temp.path().join("deployment");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");

    let generated = Command::new(env!("CARGO_BIN_EXE_webstack"))
        .args(["new", "phase-one-smoke"])
        .arg("--directory")
        .arg(&application)
        .arg("--framework-path")
        .arg(workspace)
        .output()
        .expect("webstack new should run");
    assert_success(&generated, "application generation");

    let tailwind = if cfg!(windows) {
        application.join("tools/tailwindcss.exe")
    } else {
        application.join("tools/tailwindcss")
    };
    let css = Command::new(tailwind)
        .args([
            "-i",
            "assets/css/input.css",
            "-o",
            "assets/css/app.css",
            "--minify",
        ])
        .current_dir(&application)
        .output()
        .expect("Tailwind should run");
    assert_success(&css, "CSS build");

    let release = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&application)
        .output()
        .expect("release build should run");
    assert_success(&release, "release build");

    fs::create_dir(&deployment).expect("deployment directory");
    let deployed_binary = executable(&deployment, "phase-one-smoke");
    fs::copy(
        executable(&application.join("target/release"), "phase-one-smoke"),
        &deployed_binary,
    )
    .expect("release binary should copy");

    let port = available_port();
    let config = fs::read_to_string(application.join("webstack.toml"))
        .expect("generated configuration")
        .replace("http_port = 8080", &format!("http_port = {port}"));
    fs::write(deployment.join("webstack.toml"), config).expect("deployment configuration");
    fs::create_dir(deployment.join("migrations")).expect("deployment migrations directory");
    fs::copy(
        application.join("migrations/0001_initialize.surql"),
        deployment.join("migrations/0001_initialize.surql"),
    )
    .expect("migration should copy");

    let mut deployed_files = fs::read_dir(&deployment)
        .expect("deployment directory")
        .map(|entry| entry.expect("deployment entry").file_name())
        .collect::<Vec<_>>();
    deployed_files.sort();
    assert_eq!(deployed_files.len(), 3);
    assert!(deployed_binary.is_file());
    assert!(deployment.join("webstack.toml").is_file());
    assert!(
        deployment
            .join("migrations/0001_initialize.surql")
            .is_file()
    );

    let child = Command::new(&deployed_binary)
        .current_dir(&deployment)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("release application should start");
    let mut child = ChildGuard::new(child);
    wait_for_server(port, &mut child.child);

    assert_ok_response(
        &request(port, "/").expect("root response"),
        "phase-one-smoke",
    );
    assert_ok_response(
        &request(port, "/healthz").expect("health response"),
        r#"{"status":"ok"}"#,
    );
    assert_ok_response(
        &request(port, "/static/css/app.css").expect("CSS response"),
        ".btn",
    );
    assert_ok_response(
        &request(port, "/static/js/htmx.min.js").expect("htmx response"),
        "htmx",
    );
}
