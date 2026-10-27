#![deny(unsafe_code)]

//! SQLite persistence boundary for Picker.

mod albums;
mod classifiers;
mod colors;
mod embeddings;
mod error;
mod feedback;
mod migrations;
mod models;
mod portable;
mod repositories;
mod tags;

use std::{path::Path, time::Duration};

pub use albums::*;
pub use classifiers::ClassifierRepository;
pub use colors::ColorRepository;
pub use embeddings::EmbeddingRepository;
pub use error::{Result, StoreError};
pub use feedback::FeedbackRepository;
pub use migrations::{LATEST_SCHEMA_VERSION, MIGRATIONS};
pub use models::*;
pub use portable::{PortableAssignment, PortableRepository};
pub use repositories::*;
use rusqlite::{Connection, OpenFlags};
pub use tags::*;

/// An initialized Picker SQLite database.
#[derive(Debug)]
pub struct Store {
    connection: Connection,
}

impl Store {
    /// Opens or creates a database, enables WAL and foreign keys, and migrates it.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if path.file_name().and_then(std::ffi::OsStr::to_str) == Some(":memory:") {
            return Self::open_in_memory();
        }
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
    pub fn portable(&self) -> PortableRepository<'_> {
        PortableRepository::new(&self.connection)
    }

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

    #[must_use]
    pub fn tags(&self) -> TagRepository<'_> {
        TagRepository::new(&self.connection)
    }

    #[must_use]
    pub fn tag_assignments(&self) -> TagAssignmentRepository<'_> {
        TagAssignmentRepository::new(&self.connection)
    }

    #[must_use]
    pub fn asset_filters(&self) -> AssetFilterRepository<'_> {
        AssetFilterRepository::new(&self.connection)
    }

    #[must_use]
    pub fn albums(&self) -> AlbumRepository<'_> {
        AlbumRepository::new(&self.connection)
    }

    #[must_use]
    pub fn classifiers(&self) -> ClassifierRepository<'_> {
        ClassifierRepository::new(&self.connection)
    }

    pub fn colors(&self) -> ColorRepository<'_> {
        ColorRepository::new(&self.connection)
    }

    #[must_use]
    pub fn embeddings(&self) -> EmbeddingRepository<'_> {
        EmbeddingRepository::new(&self.connection)
    }

    pub fn feedback(&self) -> FeedbackRepository<'_> {
        FeedbackRepository::new(&self.connection)
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
mod album_tests;
#[cfg(test)]
mod classifier_tests;
#[cfg(test)]
mod color_tests;
#[cfg(test)]
mod embedding_tests;
#[cfg(test)]
mod feedback_tests;
#[cfg(test)]
mod tag_tests;
#[cfg(test)]
mod tests;
