use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::{error::DatabaseError, migration::Migration};

/// Reads and validates every file from an application-owned migration directory.
pub(crate) fn filesystem_migrations(directory: &Path) -> Result<Vec<Migration>, DatabaseError> {
    let mut migrations = Vec::new();
    let mut prefixes = BTreeMap::<String, String>::new();
    let entries =
        fs::read_dir(directory).map_err(|source| DatabaseError::ReadMigrationDirectory {
            path: directory.to_owned(),
            source,
        })?;
    for entry in entries {
        let entry = entry.map_err(|source| DatabaseError::ReadMigrationDirectory {
            path: directory.to_owned(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|source| DatabaseError::ReadMigrationPath {
                path: path.clone(),
                source,
            })?;
        if !file_type.is_file() {
            return Err(DatabaseError::InvalidMigrationPath(path));
        }
        let filename = entry
            .file_name()
            .into_string()
            .map_err(|_| DatabaseError::InvalidMigrationPath(path.clone()))?;
        let prefix = validate_migration_filename(&filename)
            .map_err(|_| DatabaseError::InvalidMigrationPath(path.clone()))?;
        if let Some(first) = prefixes.insert(prefix, filename.clone()) {
            return Err(DatabaseError::DuplicateMigrationPrefix {
                first,
                second: filename,
            });
        }
        let bytes = fs::read(&path).map_err(|source| DatabaseError::ReadMigrationPath {
            path: path.clone(),
            source,
        })?;
        let source = String::from_utf8(bytes).map_err(|_| DatabaseError::NonUtf8Migration(path))?;
        if let Some(keyword) = transaction_keyword(&source) {
            return Err(DatabaseError::TransactionControl { filename, keyword });
        }
        let checksum = format!("{:x}", Sha256::digest(source.as_bytes()));
        migrations.push(Migration {
            filename,
            source,
            checksum,
        });
    }
    migrations.sort_by(|left, right| left.filename.cmp(&right.filename));
    Ok(migrations)
}

/// Checks the strict flat migration filename convention.
fn validate_migration_filename(filename: &str) -> Result<String, DatabaseError> {
    let Some(stem) = filename.strip_suffix(".surql") else {
        return Err(DatabaseError::InvalidMigrationPath(PathBuf::from(filename)));
    };
    let bytes = stem.as_bytes();
    let valid_prefix =
        bytes.len() >= 6 && bytes[..4].iter().all(u8::is_ascii_digit) && bytes[4] == b'_';
    let name = stem.get(5..).unwrap_or_default();
    let valid_name = name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && !name.ends_with('_')
        && !name.contains("__");
    if filename.contains('/') || filename.contains('\\') || !valid_prefix || !valid_name {
        return Err(DatabaseError::InvalidMigrationPath(PathBuf::from(filename)));
    }
    Ok(stem[..4].to_owned())
}

/// Finds forbidden transaction keywords while ignoring strings and comments.
fn transaction_keyword(source: &str) -> Option<&'static str> {
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' | b'"' | b'`' => index = skip_quoted(bytes, index),
            b'-' if bytes.get(index + 1) == Some(&b'-') => index = skip_line(bytes, index + 2),
            b'/' if bytes.get(index + 1) == Some(&b'/') => index = skip_line(bytes, index + 2),
            b'/' if bytes.get(index + 1) == Some(&b'*') => index = skip_block(bytes, index + 2),
            byte if byte.is_ascii_alphabetic() => {
                let start = index;
                while bytes.get(index).is_some_and(u8::is_ascii_alphabetic) {
                    index += 1;
                }
                let token = &source[start..index];
                if token.eq_ignore_ascii_case("begin") {
                    return Some("BEGIN");
                }
                if token.eq_ignore_ascii_case("commit") {
                    return Some("COMMIT");
                }
                if token.eq_ignore_ascii_case("cancel") {
                    return Some("CANCEL");
                }
            }
            _ => index += 1,
        }
    }
    None
}

/// Advances past one quoted `SurrealQL` token, including escaped characters.
fn skip_quoted(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index = (index + 2).min(bytes.len());
        } else if bytes[index] == quote {
            return index + 1;
        } else {
            index += 1;
        }
    }
    index
}

/// Advances past a line comment.
fn skip_line(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |offset| start + offset + 1)
}

/// Advances past a block comment.
fn skip_block(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .windows(2)
        .position(|window| window == b"*/")
        .map_or(bytes.len(), |offset| start + offset + 2)
}

#[cfg(test)]
mod tests {
    use super::{transaction_keyword, validate_migration_filename};

    #[test]
    fn migration_names_and_transaction_tokens_are_strict() {
        assert_eq!(
            validate_migration_filename("0001_create_users.surql").expect("valid migration"),
            "0001"
        );
        for invalid in [
            "1_create.surql",
            "0001_Create.surql",
            "0001_create-user.surql",
            "nested/0001_create.surql",
            "0001_create.sql",
        ] {
            assert!(validate_migration_filename(invalid).is_err(), "{invalid}");
        }
        assert_eq!(transaction_keyword("CREATE thing; BEGIN;"), Some("BEGIN"));
        assert_eq!(
            transaction_keyword("-- COMMIT\nCREATE thing SET note='CANCEL'"),
            None
        );
    }
}
