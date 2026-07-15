/// Verifies release assets and configures Cargo rebuild inputs.
fn main() {
    println!("cargo::rerun-if-changed=assets");
    println!("cargo::rerun-if-changed=templates");
    let release = std::env::var("PROFILE").is_ok_and(|profile| profile == "release");
    assert!(
        !release || std::path::Path::new("assets/css/app.css").is_file(),
        "assets/css/app.css is missing; run `just demo-css` before a release build"
    );
}
