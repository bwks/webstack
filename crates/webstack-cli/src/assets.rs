#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;
use thiserror::Error;
use webstack_core::config::AssetsConfig;

use crate::tools::HttpClient;

#[derive(Deserialize)]
struct AssetFile {
    #[serde(default)]
    assets: AssetsConfig,
}

#[derive(Deserialize)]
struct GithubRelease {
    assets: Vec<GithubReleaseAsset>,
}

#[derive(Deserialize)]
struct GithubReleaseAsset {
    name: String,
    digest: Option<String>,
    browser_download_url: String,
}

struct Artifact {
    path: PathBuf,
    data: Vec<u8>,
    executable: bool,
}

#[derive(Clone, Copy)]
struct Platform {
    os: &'static str,
    architecture: &'static str,
}

impl Platform {
    /// Detects the platform used by the running CLI.
    fn current() -> Result<Self, AssetError> {
        Self::new(env::consts::OS, env::consts::ARCH)
    }

    /// Validates and constructs a supported asset-tool platform.
    fn new(os: &'static str, architecture: &'static str) -> Result<Self, AssetError> {
        match (os, architecture) {
            ("linux" | "macos", "x86_64" | "aarch64") | ("windows", "x86_64") => {
                Ok(Self { os, architecture })
            }
            _ => Err(AssetError::UnsupportedPlatform { os, architecture }),
        }
    }

    /// Returns the upstream Tailwind release asset name for this platform.
    fn tailwind_asset(self) -> &'static str {
        match (self.os, self.architecture) {
            ("linux", "x86_64") => "tailwindcss-linux-x64",
            ("linux", "aarch64") => "tailwindcss-linux-arm64",
            ("macos", "x86_64") => "tailwindcss-macos-x64",
            ("macos", "aarch64") => "tailwindcss-macos-arm64",
            ("windows", "x86_64") => "tailwindcss-windows-x64.exe",
            _ => unreachable!(),
        }
    }

    /// Returns the generated application's Tailwind executable path.
    fn tailwind_path(self) -> &'static str {
        if self.os == "windows" {
            "tools/tailwindcss.exe"
        } else {
            "tools/tailwindcss"
        }
    }
}

/// A frontend asset provisioning failure.
#[derive(Debug, Error)]
pub enum AssetError {
    #[error("unsupported asset tool platform {os}/{architecture}")]
    UnsupportedPlatform {
        os: &'static str,
        architecture: &'static str,
    },
    #[error("invalid {artifact} version {version:?}")]
    InvalidVersion {
        artifact: &'static str,
        version: String,
        #[source]
        source: semver::Error,
    },
    #[error("cannot read asset configuration at {}", path.display())]
    ReadConfig {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot parse asset configuration at {}", path.display())]
    ParseConfig {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("release does not contain {0}")]
    MissingReleaseAsset(String),
    #[error("release asset {0} does not publish a SHA-256 digest")]
    MissingDigest(String),
    #[error("integrity verification failed for {0}")]
    Integrity(String),
    #[error("asset downloads failed:\n  - {}", .0.join("\n  - "))]
    DownloadFailures(Vec<String>),
    #[error("HTTP request failed")]
    Http(#[from] reqwest::Error),
    #[error("invalid upstream JSON")]
    Json(#[from] serde_json::Error),
    #[error("asset filesystem operation failed")]
    Io(#[from] io::Error),
}

impl AssetError {
    /// Adds this error to an aggregate download failure without nesting aggregates.
    fn record_download_failure(self, artifact: &str, failures: &mut Vec<String>) {
        match self {
            Self::DownloadFailures(nested) => failures.extend(nested),
            error => failures.push(format!("{artifact}: {error}")),
        }
    }
}

/// Downloads, verifies, and installs the requested frontend asset versions.
pub(crate) async fn setup(root: &Path, versions: &AssetsConfig) -> Result<(), AssetError> {
    setup_for(root, versions, Platform::current()?, &HttpClient::new()?).await
}

/// Loads frontend versions from `webstack.toml` and provisions them.
pub(crate) async fn setup_from_config(root: &Path) -> Result<(), AssetError> {
    let path = root.join("webstack.toml");
    let source = fs::read_to_string(&path).map_err(|source| AssetError::ReadConfig {
        path: path.clone(),
        source,
    })?;
    let config: AssetFile =
        toml::from_str(&source).map_err(|source| AssetError::ParseConfig { path, source })?;
    setup(root, &config.assets).await
}

/// Validates asset versions and installs every artifact as one operation.
async fn setup_for(
    root: &Path,
    versions: &AssetsConfig,
    platform: Platform,
    client: &HttpClient,
) -> Result<(), AssetError> {
    semver::Version::parse(&versions.tailwind_version).map_err(|source| {
        AssetError::InvalidVersion {
            artifact: "Tailwind CSS",
            version: versions.tailwind_version.clone(),
            source,
        }
    })?;
    semver::Version::parse(&versions.daisyui_version).map_err(|source| {
        AssetError::InvalidVersion {
            artifact: "daisyUI",
            version: versions.daisyui_version.clone(),
            source,
        }
    })?;
    semver::Version::parse(&versions.htmx_version).map_err(|source| {
        AssetError::InvalidVersion {
            artifact: "htmx",
            version: versions.htmx_version.clone(),
            source,
        }
    })?;

    let (tailwind, daisyui, htmx) = tokio::join!(
        download_tailwind(client, &versions.tailwind_version, platform),
        download_daisyui(client, &versions.daisyui_version),
        download_htmx(client, &versions.htmx_version),
    );

    let mut artifacts = Vec::new();
    let mut failures = Vec::new();
    match tailwind {
        Ok(artifact) => artifacts.push(artifact),
        Err(error) => error.record_download_failure("Tailwind CSS", &mut failures),
    }
    match daisyui {
        Ok(downloaded) => artifacts.extend(downloaded),
        Err(error) => error.record_download_failure("daisyUI", &mut failures),
    }
    match htmx {
        Ok(artifact) => artifacts.push(artifact),
        Err(error) => error.record_download_failure("htmx", &mut failures),
    }
    if !failures.is_empty() {
        return Err(AssetError::DownloadFailures(failures));
    }

    install(root, &artifacts)
}

/// Downloads and verifies the daisyUI plugin and theme modules.
async fn download_daisyui(client: &HttpClient, version: &str) -> Result<Vec<Artifact>, AssetError> {
    let url = format!("https://api.github.com/repos/saadeghi/daisyui/releases/tags/v{version}");
    let release: GithubRelease = serde_json::from_slice(&client.get(&url).await?)?;
    let (plugin, themes) = tokio::join!(
        download_release_asset(client, &release, "daisyui.js", "tools/daisyui.js", false),
        download_release_asset(
            client,
            &release,
            "daisyui-theme.js",
            "tools/daisyui-theme.js",
            false,
        ),
    );

    let mut artifacts = Vec::new();
    let mut failures = Vec::new();
    match plugin {
        Ok(artifact) => artifacts.push(artifact),
        Err(error) => error.record_download_failure("daisyui.js", &mut failures),
    }
    match themes {
        Ok(artifact) => artifacts.push(artifact),
        Err(error) => error.record_download_failure("daisyui-theme.js", &mut failures),
    }
    if failures.is_empty() {
        Ok(artifacts)
    } else {
        Err(AssetError::DownloadFailures(failures))
    }
}

/// Downloads and verifies the Tailwind standalone executable.
async fn download_tailwind(
    client: &HttpClient,
    version: &str,
    platform: Platform,
) -> Result<Artifact, AssetError> {
    let url =
        format!("https://api.github.com/repos/tailwindlabs/tailwindcss/releases/tags/v{version}");
    let release: GithubRelease = serde_json::from_slice(&client.get(&url).await?)?;
    download_release_asset(
        client,
        &release,
        platform.tailwind_asset(),
        platform.tailwind_path(),
        true,
    )
    .await
}

/// Downloads and verifies the configured htmx release asset.
async fn download_htmx(client: &HttpClient, version: &str) -> Result<Artifact, AssetError> {
    let url = format!("https://api.github.com/repos/bigskysoftware/htmx/releases/tags/v{version}");
    let release: GithubRelease = serde_json::from_slice(&client.get(&url).await?)?;
    download_release_asset(
        client,
        &release,
        "htmx.min.js",
        "assets/js/htmx.min.js",
        false,
    )
    .await
}

/// Resolves, downloads, and verifies one named GitHub release asset.
async fn download_release_asset(
    client: &HttpClient,
    release: &GithubRelease,
    name: &str,
    destination: &str,
    executable: bool,
) -> Result<Artifact, AssetError> {
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .ok_or_else(|| AssetError::MissingReleaseAsset(name.to_owned()))?;
    let digest = asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .ok_or_else(|| AssetError::MissingDigest(name.to_owned()))?;
    let data = client.get(&asset.browser_download_url).await?;
    verify_sha256(name, &data, digest)?;
    Ok(Artifact {
        path: PathBuf::from(destination),
        data,
        executable,
    })
}

/// Verifies artifact bytes against a hexadecimal SHA-256 digest.
fn verify_sha256(name: &str, data: &[u8], expected: &str) -> Result<(), AssetError> {
    let actual = format!("{:x}", Sha256::digest(data));
    if actual == expected {
        Ok(())
    } else {
        Err(AssetError::Integrity(name.to_owned()))
    }
}

/// Writes artifacts to staging before committing them to the application.
fn install(root: &Path, artifacts: &[Artifact]) -> Result<(), AssetError> {
    let staging = tempfile::Builder::new()
        .prefix(".webstack-assets-")
        .tempdir_in(root)?;
    for artifact in artifacts {
        let path = staging.path().join(&artifact.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &artifact.data)?;
        set_executable(&path, artifact.executable)?;
    }
    commit_staging(root, &staging, artifacts)
}

/// Moves every staged artifact into its final application path.
fn commit_staging(
    root: &Path,
    staging: &TempDir,
    artifacts: &[Artifact],
) -> Result<(), AssetError> {
    for artifact in artifacts {
        let source = staging.path().join(&artifact.path);
        let destination = root.join(&artifact.path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        if destination.exists() {
            fs::remove_file(&destination)?;
        }
        fs::rename(source, destination)?;
    }
    Ok(())
}

#[cfg(unix)]
/// Applies executable permissions to a Unix artifact when requested.
fn set_executable(path: &Path, executable: bool) -> io::Result<()> {
    if executable {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

#[cfg(not(unix))]
/// Leaves permissions unchanged on platforms without Unix permission bits.
fn set_executable(_path: &Path, _executable: bool) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use sha2::Digest as _;

    use super::{AssetError, Platform, verify_sha256};

    #[test]
    fn maps_all_supported_platforms() {
        let cases = [
            ("linux", "x86_64", "tailwindcss-linux-x64"),
            ("linux", "aarch64", "tailwindcss-linux-arm64"),
            ("macos", "x86_64", "tailwindcss-macos-x64"),
            ("macos", "aarch64", "tailwindcss-macos-arm64"),
            ("windows", "x86_64", "tailwindcss-windows-x64.exe"),
        ];
        for (os, architecture, expected) in cases {
            assert_eq!(
                Platform::new(os, architecture)
                    .expect("supported")
                    .tailwind_asset(),
                expected
            );
        }
        assert!(matches!(
            Platform::new("windows", "aarch64"),
            Err(AssetError::UnsupportedPlatform { .. })
        ));
    }

    #[test]
    fn verifies_published_digests() {
        let bytes = b"verified artifact";
        let sha256 = format!("{:x}", sha2::Sha256::digest(bytes));
        verify_sha256("artifact", bytes, &sha256).expect("valid SHA-256");
        assert!(matches!(
            verify_sha256("artifact", b"tampered", &sha256),
            Err(AssetError::Integrity(name)) if name == "artifact"
        ));
    }

    #[test]
    fn download_failures_are_flattened_and_readable() {
        let mut failures = Vec::new();
        AssetError::DownloadFailures(vec![
            "daisyui.js: request failed".to_owned(),
            "daisyui-theme.js: digest failed".to_owned(),
        ])
        .record_download_failure("daisyUI", &mut failures);
        AssetError::Integrity("htmx.min.js".to_owned())
            .record_download_failure("htmx", &mut failures);

        let error = AssetError::DownloadFailures(failures);
        assert_eq!(
            error.to_string(),
            "asset downloads failed:\n  - daisyui.js: request failed\n  - daisyui-theme.js: digest failed\n  - htmx: integrity verification failed for htmx.min.js"
        );
    }
}
