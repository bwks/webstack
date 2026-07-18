use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::scaffold;

const FRAMEWORK_GIT: &str = "https://github.com/bwks/webstack.git";
const FRAMEWORK_BRANCH: &str = "framework-baseline";

#[derive(Debug)]
pub(crate) struct GenerateOptions {
    pub(crate) name: String,
    pub(crate) target: PathBuf,
    pub(crate) framework_path: Option<PathBuf>,
}

/// An application generation failure.
#[derive(Debug, Error)]
pub enum GenerateError {
    /// The package name does not follow Webstack's naming convention.
    #[error("application name {0:?} must be lowercase kebab-case")]
    InvalidName(String),

    /// The generator refuses to write into any existing filesystem entry.
    #[error("target already exists: {}", .0.display())]
    TargetExists(PathBuf),

    /// Scaffold files could not be written completely.
    #[error("cannot write scaffold: {source}")]
    WriteScaffold {
        #[source]
        source: io::Error,
    },
}

#[tracing::instrument(
    name = "generate_application",
    skip_all,
    fields(app.name = %options.name, target = %options.target.display())
)]
/// Generates a new application without invoking external development tools.
pub(crate) fn generate(options: &GenerateOptions) -> Result<PathBuf, GenerateError> {
    validate_name(&options.name)?;
    if options.target.exists() {
        return Err(GenerateError::TargetExists(options.target.clone()));
    }

    fs::create_dir_all(&options.target)
        .map_err(|source| GenerateError::WriteScaffold { source })?;
    if let Err(source) = write_scaffold(options) {
        let _cleanup_result = fs::remove_dir_all(&options.target);
        return Err(GenerateError::WriteScaffold { source });
    }
    tracing::debug!("application scaffold written");

    Ok(options.target.clone())
}

/// Renders and writes every file in the application scaffold.
fn write_scaffold(options: &GenerateOptions) -> io::Result<()> {
    let dependency = framework_dependency(options.framework_path.as_deref())?;
    for file in scaffold::FILES {
        let relative_path = file.path.replace("{{app_name}}", &options.name);
        let path = options.target.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = file
            .contents
            .replace("{{app_name}}", &options.name)
            .replace("{{framework_dependency}}", &dependency);
        fs::write(path, contents)?;
    }
    for file in scaffold::BINARY_FILES {
        let path = options.target.join(file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, file.contents)?;
    }
    Ok(())
}

/// Validates Webstack's lowercase kebab-case application naming convention.
fn validate_name(name: &str) -> Result<(), GenerateError> {
    let valid = !name.is_empty()
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !name.ends_with('-')
        && !name.contains("--");
    if valid {
        Ok(())
    } else {
        Err(GenerateError::InvalidName(name.to_owned()))
    }
}

/// Builds the generated manifest entry for the Webstack facade crate.
fn framework_dependency(path: Option<&Path>) -> io::Result<String> {
    if let Some(path) = path {
        let root = path.canonicalize()?;
        let crate_path = if root.join("crates/webstack/Cargo.toml").is_file() {
            root.join("crates/webstack")
        } else {
            root
        };
        let escaped = crate_path.display().to_string().replace('\\', "\\\\");
        Ok(format!("{{ path = \"{escaped}\" }}"))
    } else {
        Ok(format!(
            "{{ git = \"{FRAMEWORK_GIT}\", branch = \"{FRAMEWORK_BRANCH}\" }}"
        ))
    }
}
