use std::collections::BTreeMap;

use rusqlite::{params, Connection};

use crate::{Result, StoreError};

#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: include_str!("../migrations/0001_initial_schema.sql"),
    },
    Migration {
        version: 2,
        name: "asset_exif",
        sql: include_str!("../migrations/0002_asset_exif.sql"),
    },
    Migration {
        version: 3,
        name: "tags",
        sql: include_str!("../migrations/0003_tags.sql"),
    },
    Migration {
        version: 4,
        name: "asset_colors",
        sql: include_str!("../migrations/0004_asset_colors.sql"),
    },
    Migration {
        version: 5,
        name: "albums",
        sql: include_str!("../migrations/0005_albums.sql"),
    },
    Migration {
        version: 6,
        name: "embeddings",
        sql: include_str!("../migrations/0006_embeddings.sql"),
    },
    Migration {
        version: 7,
        name: "feedback_events",
        sql: include_str!("../migrations/0007_feedback_events.sql"),
    },
    Migration {
        version: 8,
        name: "custom_classifiers",
        sql: include_str!("../migrations/0008_custom_classifiers.sql"),
    },
    Migration {
        version: 9,
        name: "pending_assignments",
        sql: include_str!("../migrations/0009_pending_assignments.sql"),
    },
];

pub const LATEST_SCHEMA_VERSION: u32 = 9;

pub(crate) fn migrate(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY CHECK (version > 0),
            name TEXT NOT NULL UNIQUE,
            checksum TEXT NOT NULL,
            applied_at INTEGER NOT NULL DEFAULT (unixepoch())
        ) STRICT;",
    )?;

    let applied = applied_migrations(connection)?;
    for version in applied.keys() {
        if !MIGRATIONS
            .iter()
            .any(|migration| migration.version == *version)
        {
            return Err(StoreError::UnknownMigration(*version));
        }
    }

    for migration in MIGRATIONS {
        let checksum = checksum(migration.sql);
        if let Some((name, recorded_checksum)) = applied.get(&migration.version) {
            if name != migration.name || recorded_checksum != &checksum {
                return Err(StoreError::MigrationModified {
                    version: migration.version,
                    name: migration.name,
                });
            }
            continue;
        }

        let transaction = connection.unchecked_transaction()?;
        transaction.execute_batch(migration.sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations (version, name, checksum) VALUES (?1, ?2, ?3)",
            params![migration.version, migration.name, checksum],
        )?;
        transaction.commit()?;
    }
    Ok(())
}

pub(crate) fn schema_version(connection: &Connection) -> Result<u32> {
    Ok(connection.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?)
}

fn applied_migrations(connection: &Connection) -> Result<BTreeMap<u32, (String, String)>> {
    let mut statement = connection
        .prepare("SELECT version, name, checksum FROM schema_migrations ORDER BY version")?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    let mut applied = BTreeMap::new();
    for row in rows {
        let (version, name, checksum) = row?;
        applied.insert(version, (name, checksum));
    }
    Ok(applied)
}

// Stable FNV-1a detects accidental edits; it is not a security boundary.
fn checksum(sql: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in sql.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}
