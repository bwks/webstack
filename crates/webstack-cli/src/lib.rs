#![doc = "Command-line tooling for generating and developing Webstack applications."]

mod generator;
mod scaffold;

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use generator::GenerateOptions;
use thiserror::Error;

pub use generator::GenerateError;

#[derive(Debug, Parser)]
#[command(
    name = "webstack",
    version,
    about = "Webstack application framework",
    arg_required_else_help = true
)]
pub struct Cli {
    /// Increase diagnostic verbosity. Repeat for trace-level diagnostics.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

impl Cli {
    /// Parses the current process arguments with Clap.
    #[must_use]
    pub fn parse_args() -> Self {
        Self::parse()
    }

    /// Returns how many times the verbosity flag was supplied.
    #[must_use]
    pub const fn verbosity(&self) -> u8 {
        self.verbose
    }

    /// Executes the parsed command.
    ///
    /// # Errors
    ///
    /// Returns a typed operational error when the selected command fails.
    pub fn execute(self) -> Result<(), CliError> {
        execute(self.command)
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate an independent Webstack application.
    New(NewArgs),
    /// Manage frontend asset tooling.
    Assets {
        #[command(subcommand)]
        command: AssetCommand,
    },
    /// Generate application source files.
    Generate {
        #[command(subcommand)]
        command: GenerateCommand,
    },
    /// Check the development and runtime environment.
    Doctor,
}

#[derive(Debug, Args)]
struct NewArgs {
    /// Lowercase kebab-case application name.
    name: String,

    /// Complete destination path. Defaults to ./<name>.
    #[arg(long, value_name = "PATH")]
    directory: Option<PathBuf>,

    /// Use a local Webstack checkout instead of the Git dependency.
    #[arg(long, value_name = "PATH", hide = true)]
    framework_path: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum AssetCommand {
    /// Download the standalone Tailwind CLI.
    Setup,
}

#[derive(Debug, Subcommand)]
enum GenerateCommand {
    /// Generate a database migration.
    Migration { name: String },
}

/// Parses process arguments and executes the selected command.
///
/// # Errors
///
/// Returns an error when an operational command cannot be completed. Clap
/// reports argument parsing errors directly before this function returns.
pub fn run() -> Result<(), CliError> {
    Cli::parse_args().execute()
}

fn execute(command: Command) -> Result<(), CliError> {
    match command {
        Command::New(args) => {
            let target = args.directory.unwrap_or_else(|| PathBuf::from(&args.name));
            let options = GenerateOptions {
                name: args.name,
                target,
                framework_path: args.framework_path,
            };
            let generated = generator::generate(&options)?;
            println!(
                "Created {}\n\nNext steps:\n  cd {}\n  git init -b main\n  cargo check",
                generated.display(),
                generated.display()
            );
            Ok(())
        }
        Command::Assets { command } => not_implemented(match command {
            AssetCommand::Setup => "assets setup",
        }),
        Command::Generate { command } => not_implemented(match command {
            GenerateCommand::Migration { name: _ } => "generate migration",
        }),
        Command::Doctor => not_implemented("doctor"),
    }
}

fn not_implemented(command: &'static str) -> Result<(), CliError> {
    Err(CliError::NotImplemented(command))
}

/// A typed operational failure returned by the Webstack CLI library.
#[derive(Debug, Error)]
pub enum CliError {
    /// Application generation failed.
    #[error(transparent)]
    Generate(#[from] GenerateError),

    /// The requested command is part of a later milestone.
    #[error("webstack {0}: not implemented yet")]
    NotImplemented(&'static str),
}
