use std::{error::Error, fmt};

/// Typed failures from database setup, validation, migration, and queries.
#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    InvalidInput { field: &'static str, reason: String },
    CorruptData { field: &'static str, value: String },
    MigrationModified { version: u32, name: &'static str },
    UnknownMigration(u32),
    Configuration(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "SQLite error: {error}"),
            Self::InvalidInput { field, reason } => write!(formatter, "invalid {field}: {reason}"),
            Self::CorruptData { field, value } => {
                write!(formatter, "database contains invalid {field}: {value}")
            }
            Self::MigrationModified { version, name } => {
                write!(
                    formatter,
                    "applied migration {version} ({name}) was modified"
                )
            }
            Self::UnknownMigration(version) => {
                write!(formatter, "database contains unknown migration {version}")
            }
            Self::Configuration(message) => formatter.write_str(message),
        }
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;
