use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use thiserror::Error;

use crate::scaffold;

const FRAMEWORK_GIT: &str = "https://github.com/bwks/webstack.git";
const FRAMEWORK_BRANCH: &str = "framework-baseline";

#[derive(Debug)]
pub(crate) struct GenerateOptions {
    pub(crate) name: String,
    pub(crate) target: PathBuf,
    pub(crate) initialize_git: bool,
    pub(crate) generate_lockfile: bool,
    pub(crate) framework_path: Option<PathBuf>,
}

pub(crate) trait Runner {
    fn succeeds<I, S>(&self, program: &str, args: I, directory: &Path) -> io::Result<bool>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SystemRunner;

impl Runner for SystemRunner {
    fn succeeds<I, S>(&self, program: &str, args: I, directory: &Path) -> io::Result<bool>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Command::new(program)
            .args(args)
            .current_dir(directory)
            .status()
            .map(|status| status.success())
    }
}

#[derive(Debug)]
pub(crate) struct Generator<R> {
    runner: R,
}

impl<R: Runner> Generator<R> {
    pub(crate) const fn new(runner: R) -> Self {
        Self { runner }
    }

    #[tracing::instrument(
        name = "generate_application",
        skip_all,
        fields(app.name = %options.name, target = %options.target.display())
    )]
    pub(crate) fn generate(&self, options: &GenerateOptions) -> Result<PathBuf, GenerateError> {
        validate_name(&options.name)?;
        if options.target.exists() {
            return Err(GenerateError::TargetExists(options.target.clone()));
        }

        fs::create_dir_all(&options.target)
            .map_err(|source| GenerateError::WriteScaffold { source })?;
        if let Err(source) = Self::write_scaffold(options) {
            let _cleanup_result = fs::remove_dir_all(&options.target);
            return Err(GenerateError::WriteScaffold { source });
        }
        tracing::debug!("application scaffold written");

        if options.generate_lockfile {
            tracing::debug!("resolving application dependencies");
            let succeeded = self
                .runner
                .succeeds("cargo", ["generate-lockfile"], &options.target)
                .map_err(|source| GenerateError::StartProcess {
                    process: "cargo generate-lockfile",
                    source,
                    target: options.target.clone(),
                })?;
            if !succeeded {
                return Err(GenerateError::ProcessFailed {
                    process: "cargo generate-lockfile",
                    retry: "cargo generate-lockfile",
                    target: options.target.clone(),
                });
            }
            tracing::info!("application lockfile generated");
        }

        if options.initialize_git {
            tracing::debug!("initializing application Git repository");
            let succeeded = self
                .runner
                .succeeds("git", ["init", "-b", "main"], &options.target)
                .map_err(|source| GenerateError::StartProcess {
                    process: "git init",
                    source,
                    target: options.target.clone(),
                })?;
            if !succeeded {
                return Err(GenerateError::ProcessFailed {
                    process: "git init",
                    retry: "git init -b main",
                    target: options.target.clone(),
                });
            }
            tracing::info!("application Git repository initialized");
        }

        Ok(options.target.clone())
    }

    fn write_scaffold(options: &GenerateOptions) -> io::Result<()> {
        let dependency = framework_dependency(options.framework_path.as_deref())?;
        for file in scaffold::FILES {
            let path = options.target.join(file.path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let contents = file
                .contents
                .replace("{{app_name}}", &options.name)
                .replace("{{framework_dependency}}", &dependency);
            fs::write(path, contents)?;
        }
        Ok(())
    }
}

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

    /// An external process could not be started after the scaffold was written.
    #[error(
        "could not start {process}: {source}; generated project preserved at {}",
        target.display()
    )]
    StartProcess {
        process: &'static str,
        #[source]
        source: io::Error,
        target: PathBuf,
    },

    /// An external process returned an unsuccessful status.
    #[error(
        "{process} failed; generated project preserved at {}; retry with `{retry}` from that directory",
        target.display()
    )]
    ProcessFailed {
        process: &'static str,
        retry: &'static str,
        target: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use std::{error::Error, io};

    use tempfile::TempDir;

    use super::{GenerateOptions, Generator, Runner};

    #[derive(Debug)]
    struct FailingRunner;

    #[derive(Debug)]
    struct UnavailableRunner;

    impl Runner for FailingRunner {
        fn succeeds<I, S>(
            &self,
            _program: &str,
            _args: I,
            _directory: &std::path::Path,
        ) -> io::Result<bool>
        where
            I: IntoIterator<Item = S>,
            S: AsRef<std::ffi::OsStr>,
        {
            Ok(false)
        }
    }

    impl Runner for UnavailableRunner {
        fn succeeds<I, S>(
            &self,
            _program: &str,
            _args: I,
            _directory: &std::path::Path,
        ) -> io::Result<bool>
        where
            I: IntoIterator<Item = S>,
            S: AsRef<std::ffi::OsStr>,
        {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "process unavailable",
            ))
        }
    }

    #[test]
    fn dependency_failure_preserves_the_scaffold_and_explains_recovery() {
        let temp = TempDir::new().expect("temporary directory");
        let target = temp.path().join("inventory-app");
        let options = GenerateOptions {
            name: "inventory-app".to_owned(),
            target: target.clone(),
            initialize_git: true,
            generate_lockfile: true,
            framework_path: None,
        };

        let error = Generator::new(FailingRunner)
            .generate(&options)
            .expect_err("lockfile generation should fail");

        assert!(target.join("Cargo.toml").is_file());
        assert!(target.join("src/main.rs").is_file());
        assert!(!target.join(".git").exists());
        let message = error.to_string();
        assert!(message.contains("generated project preserved"));
        assert!(message.contains("cargo generate-lockfile"));
    }

    #[test]
    fn process_start_failure_retains_its_source() {
        let temp = TempDir::new().expect("temporary directory");
        let target = temp.path().join("inventory-app");
        let options = GenerateOptions {
            name: "inventory-app".to_owned(),
            target,
            initialize_git: false,
            generate_lockfile: true,
            framework_path: None,
        };

        let error = Generator::new(UnavailableRunner)
            .generate(&options)
            .expect_err("process should be unavailable");

        assert_eq!(
            error
                .source()
                .expect("source should be retained")
                .to_string(),
            "process unavailable"
        );
    }
}
