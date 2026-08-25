use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use tempfile::NamedTempFile;
use thiserror::Error;

const MIGRATIONS_DIRECTORY: &str = "migrations";
const MAX_MIGRATION_NUMBER: u16 = 9_999;

/// A migration source generation failure.
#[derive(Debug, Error)]
pub enum MigrationError {
    /// The requested migration name is not lowercase `snake_case`.
    #[error("migration name {0:?} must be lowercase snake_case and begin with a letter")]
    InvalidName(String),
    /// The application migration directory is unavailable.
    #[error("cannot inspect migrations directory {}", path.display())]
    ReadDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// An existing directory entry violates the migration filename convention.
    #[error("invalid existing migration path {}; expected NNNN_lowercase_name.sql", .0.display())]
    InvalidExistingMigration(PathBuf),
    /// A migration with this descriptive name already exists.
    #[error("migration name {name:?} already exists at {}", path.display())]
    NameCollision { name: String, path: PathBuf },
    /// No additional four-digit migration number is available.
    #[error("migration number overflow after 9999")]
    NumberOverflow,
    /// The computed migration target already exists.
    #[error("migration target already exists: {}", .0.display())]
    TargetExists(PathBuf),
    /// The migration file could not be created atomically.
    #[error("cannot create migration {}", path.display())]
    Create {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Generates the next numbered migration in the current application's directory.
///
/// # Errors
///
/// Returns a typed error for invalid names, malformed history, collisions,
/// numbering overflow, or filesystem failures.
pub fn generate(root: &Path, name: &str) -> Result<PathBuf, MigrationError> {
    validate_name(name)?;
    let directory = root.join(MIGRATIONS_DIRECTORY);
    let entries = fs::read_dir(&directory).map_err(|source| MigrationError::ReadDirectory {
        path: directory.clone(),
        source,
    })?;
    let mut highest = 0_u16;
    for entry in entries {
        let entry = entry.map_err(|source| MigrationError::ReadDirectory {
            path: directory.clone(),
            source,
        })?;
        let path = entry.path();
        let filename = entry
            .file_name()
            .into_string()
            .map_err(|_| MigrationError::InvalidExistingMigration(path.clone()))?;
        let (number, existing_name) = parse_filename(&filename)
            .ok_or_else(|| MigrationError::InvalidExistingMigration(path.clone()))?;
        if existing_name == name {
            return Err(MigrationError::NameCollision {
                name: name.to_owned(),
                path,
            });
        }
        highest = highest.max(number);
    }
    let number = highest
        .checked_add(1)
        .filter(|number| *number <= MAX_MIGRATION_NUMBER)
        .ok_or(MigrationError::NumberOverflow)?;
    let target = directory.join(format!("{number:04}_{name}.sql"));
    if target.exists() {
        return Err(MigrationError::TargetExists(target));
    }

    let mut temporary =
        NamedTempFile::new_in(&directory).map_err(|source| MigrationError::Create {
            path: target.clone(),
            source,
        })?;
    writeln!(temporary, "-- Migration: {name}\n").map_err(|source| MigrationError::Create {
        path: target.clone(),
        source,
    })?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|source| MigrationError::Create {
            path: target.clone(),
            source,
        })?;
    temporary.persist_noclobber(&target).map_err(|error| {
        if error.error.kind() == io::ErrorKind::AlreadyExists {
            MigrationError::TargetExists(target.clone())
        } else {
            MigrationError::Create {
                path: target.clone(),
                source: error.error,
            }
        }
    })?;
    Ok(target)
}

/// Validates a requested lowercase `snake_case` migration name.
fn validate_name(name: &str) -> Result<(), MigrationError> {
    let valid = name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && !name.ends_with('_')
        && !name.contains("__");
    if valid {
        Ok(())
    } else {
        Err(MigrationError::InvalidName(name.to_owned()))
    }
}

/// Parses one strict four-digit migration filename.
fn parse_filename(filename: &str) -> Option<(u16, &str)> {
    let stem = filename.strip_suffix(".sql")?;
    let bytes = stem.as_bytes();
    if bytes.len() < 6 || !bytes[..4].iter().all(u8::is_ascii_digit) || bytes[4] != b'_' {
        return None;
    }
    let name = stem.get(5..)?;
    validate_name(name).ok()?;
    Some((stem[..4].parse().ok()?, name))
}

#[cfg(test)]
mod tests {
    use super::{parse_filename, validate_name};

    #[test]
    fn validates_names_and_existing_filenames() {
        assert!(validate_name("create_users").is_ok());
        assert!(validate_name("create_users2").is_ok());
        for invalid in ["", "Create_users", "2_users", "users-roles", "users__roles"] {
            assert!(validate_name(invalid).is_err(), "{invalid}");
        }
        assert_eq!(
            parse_filename("0042_create_users.sql"),
            Some((42, "create_users"))
        );
        assert_eq!(parse_filename("42_create_users.sql"), None);
    }
}
