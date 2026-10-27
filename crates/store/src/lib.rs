#![deny(unsafe_code)]

//! SQLite persistence boundary for Picker.

mod error;
mod migrations;
mod models;
mod repositories;

use std::{path::Path, time::Duration};

pub use error::{Result, StoreError};
pub use migrations::{LATEST_SCHEMA_VERSION, MIGRATIONS};
pub use models::*;
pub use repositories::*;
use rusqlite::{Connection, OpenFlags};

/// An initialized Picker SQLite database.
#[derive(Debug)]
pub struct Store {
    connection: Connection,
}

impl Store {
    /// Opens or creates a database, enables WAL and foreign keys, and migrates it.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        configure(&connection, true)?;
        migrations::migrate(&connection)?;
        Ok(Self { connection })
    }

    /// Opens an isolated in-memory database for tests and ephemeral operations.
    /// SQLite does not support WAL for in-memory databases.
    pub fn open_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        configure(&connection, false)?;
        migrations::migrate(&connection)?;
        Ok(Self { connection })
    }

    pub fn schema_version(&self) -> Result<u32> {
        migrations::schema_version(&self.connection)
    }

    #[must_use]
    pub fn roots(&self) -> RootRepository<'_> {
        RootRepository::new(&self.connection)
    }

    #[must_use]
    pub fn assets(&self) -> AssetRepository<'_> {
        AssetRepository::new(&self.connection)
    }

    #[must_use]
    pub fn locations(&self) -> LocationRepository<'_> {
        LocationRepository::new(&self.connection)
    }

    #[must_use]
    pub fn stage_states(&self) -> StageStateRepository<'_> {
        StageStateRepository::new(&self.connection)
    }

    #[must_use]
    pub fn jobs(&self) -> JobRepository<'_> {
        JobRepository::new(&self.connection)
    }

    #[must_use]
    pub fn settings(&self) -> SettingRepository<'_> {
        SettingRepository::new(&self.connection)
    }

    /// Provides transaction access for later use-case orchestration.
    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
}

fn configure(connection: &Connection, require_wal: bool) -> Result<()> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    if require_wal {
        let mode: String =
            connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(StoreError::Configuration(format!(
                "SQLite refused WAL mode and selected {mode}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
