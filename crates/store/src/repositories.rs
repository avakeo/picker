use std::{path::Component, str::FromStr};

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    Asset, Job, JobStatus, Location, LocationStatus, NewAsset, NewLocation, NewRoot, Result, Root,
    RootStatus, Setting, Stage, StageState, StageStatus, StageTarget, StoreError,
};

#[derive(Debug, Clone, Copy)]
pub struct RootRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> RootRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn insert(&self, root: &NewRoot<'_>) -> Result<Root> {
        validate_absolute_path(root.absolute_path)?;
        validate_nonempty("display_name", root.display_name)?;
        self.connection.execute(
            "INSERT INTO roots (absolute_path, volume_id, display_name, enabled, status)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                root.absolute_path,
                root.volume_id,
                root.display_name,
                root.enabled,
                root.status.as_str()
            ],
        )?;
        self.get(self.connection.last_insert_rowid())
    }

    pub fn get(&self, id: i64) -> Result<Root> {
        Ok(self.connection.query_row(
            "SELECT id, absolute_path, volume_id, display_name, enabled, status
             FROM roots WHERE id = ?1",
            [id],
            root_from_row,
        )?)
    }

    pub fn list(&self) -> Result<Vec<Root>> {
        let mut statement = self.connection.prepare(
            "SELECT id, absolute_path, volume_id, display_name, enabled, status
             FROM roots ORDER BY id",
        )?;
        let rows = statement.query_map([], root_from_row)?;
        collect(rows)
    }

    pub fn delete(&self, id: i64) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM roots WHERE id = ?1", [id])?
            == 1)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AssetRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> AssetRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn insert(&self, asset: &NewAsset<'_>) -> Result<Asset> {
        let file_size = i64::try_from(asset.file_size).map_err(|_| StoreError::InvalidInput {
            field: "file_size",
            reason: "must fit SQLite's signed 64-bit INTEGER".to_owned(),
        })?;
        self.connection.execute(
            "INSERT INTO assets
                (blake3_hash, file_size, width, height, format, captured_at, perceptual_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                asset.blake3_hash.as_slice(),
                file_size,
                asset.width,
                asset.height,
                asset.format,
                asset.captured_at,
                asset.perceptual_hash.map(<[u8; 8]>::as_slice)
            ],
        )?;
        self.get(self.connection.last_insert_rowid())
    }

    pub fn get(&self, id: i64) -> Result<Asset> {
        let raw = self.connection.query_row(
            "SELECT id, blake3_hash, file_size, width, height, format, captured_at, perceptual_hash
             FROM assets WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<u32>>(3)?,
                    row.get::<_, Option<u32>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<Vec<u8>>>(7)?,
                ))
            },
        )?;
        let file_size = u64::try_from(raw.2).map_err(|_| corrupt("file_size", raw.2))?;
        Ok(Asset {
            id: raw.0,
            blake3_hash: fixed_blob("blake3_hash", raw.1)?,
            file_size,
            width: raw.3,
            height: raw.4,
            format: raw.5,
            captured_at: raw.6,
            perceptual_hash: raw
                .7
                .map(|value| fixed_blob("perceptual_hash", value))
                .transpose()?,
        })
    }

    pub fn find_by_hash(&self, hash: &[u8; 32]) -> Result<Option<Asset>> {
        let id = self
            .connection
            .query_row(
                "SELECT id FROM assets WHERE blake3_hash = ?1",
                [hash.as_slice()],
                |row| row.get(0),
            )
            .optional()?;
        id.map(|value| self.get(value)).transpose()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LocationRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> LocationRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn insert(&self, location: &NewLocation<'_>) -> Result<Location> {
        validate_relative_path(location.relative_path)?;
        self.connection.execute(
            "INSERT INTO locations (asset_id, root_id, relative_path, modified_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                location.asset_id,
                location.root_id,
                location.relative_path,
                location.modified_at,
                location.status.as_str()
            ],
        )?;
        self.get(self.connection.last_insert_rowid())
    }

    pub fn get(&self, id: i64) -> Result<Location> {
        let raw = self.connection.query_row(
            "SELECT id, asset_id, root_id, relative_path, modified_at, status
             FROM locations WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )?;
        Ok(Location {
            id: raw.0,
            asset_id: raw.1,
            root_id: raw.2,
            relative_path: raw.3,
            modified_at: raw.4,
            status: LocationStatus::from_str(&raw.5)?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StageStateRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> StageStateRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn upsert(
        &self,
        target: StageTarget,
        stage: Stage,
        status: StageStatus,
        version: &str,
        error: Option<&str>,
    ) -> Result<StageState> {
        validate_nonempty("stage version", version)?;
        if status != StageStatus::Failed && error.is_some() {
            return Err(StoreError::InvalidInput {
                field: "error",
                reason: "is only valid for a failed stage".to_owned(),
            });
        }
        let (target_column, target_id) = target_parts(target);
        let sql = format!(
            "INSERT INTO stage_states ({target_column}, stage, status, version, error)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT ({target_column}, stage) WHERE {target_column} IS NOT NULL
             DO UPDATE SET status = excluded.status, version = excluded.version,
                           error = excluded.error, updated_at = unixepoch()"
        );
        self.connection.execute(
            &sql,
            params![target_id, stage.as_str(), status.as_str(), version, error],
        )?;
        self.get(target, stage)?
            .ok_or_else(|| StoreError::CorruptData {
                field: "stage_state",
                value: "upserted row was not found".to_owned(),
            })
    }

    pub fn get(&self, target: StageTarget, stage: Stage) -> Result<Option<StageState>> {
        let (target_column, target_id) = target_parts(target);
        let sql = format!(
            "SELECT id, asset_id, location_id, stage, status, version, error, updated_at
             FROM stage_states WHERE {target_column} = ?1 AND stage = ?2"
        );
        let raw = self
            .connection
            .query_row(
                &sql,
                params![target_id, stage.as_str()],
                stage_state_from_row,
            )
            .optional()?;
        raw.map(stage_state_from_raw).transpose()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct JobRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> JobRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn enqueue(
        &self,
        target: StageTarget,
        stage: Stage,
        priority: i64,
        max_attempts: u32,
    ) -> Result<Job> {
        if max_attempts == 0 {
            return Err(StoreError::InvalidInput {
                field: "max_attempts",
                reason: "must be greater than zero".to_owned(),
            });
        }
        let (asset_id, location_id) = target_ids(target);
        self.connection.execute(
            "INSERT INTO jobs
                (asset_id, location_id, stage, priority, max_attempts)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                asset_id,
                location_id,
                stage.as_str(),
                priority,
                max_attempts
            ],
        )?;
        self.get(self.connection.last_insert_rowid())
    }

    pub fn get(&self, id: i64) -> Result<Job> {
        let raw = self.connection.query_row(
            "SELECT id, asset_id, location_id, stage, priority, attempts,
                    max_attempts, status, last_error, created_at, updated_at
             FROM jobs WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get::<_, String>(3)?,
                    row.get(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                ))
            },
        )?;
        Ok(Job {
            id: raw.0,
            target: parse_target(raw.1, raw.2)?,
            stage: Stage::from_str(&raw.3)?,
            priority: raw.4,
            attempts: u32::try_from(raw.5).map_err(|_| corrupt("attempts", raw.5))?,
            max_attempts: u32::try_from(raw.6).map_err(|_| corrupt("max_attempts", raw.6))?,
            status: JobStatus::from_str(&raw.7)?,
            last_error: raw.8,
            created_at: raw.9,
            updated_at: raw.10,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SettingRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> SettingRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn set(&self, key: &str, value: &str) -> Result<Setting> {
        validate_nonempty("setting key", key)?;
        self.connection.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = unixepoch()",
            params![key, value],
        )?;
        Ok(Setting {
            key: key.to_owned(),
            value: value.to_owned(),
        })
    }

    pub fn get(&self, key: &str) -> Result<Option<Setting>> {
        Ok(self
            .connection
            .query_row(
                "SELECT key, value FROM settings WHERE key = ?1",
                [key],
                |row| {
                    Ok(Setting {
                        key: row.get(0)?,
                        value: row.get(1)?,
                    })
                },
            )
            .optional()?)
    }
}

fn root_from_row(row: &Row<'_>) -> rusqlite::Result<Root> {
    let status: String = row.get(5)?;
    Ok(Root {
        id: row.get(0)?,
        absolute_path: row.get(1)?,
        volume_id: row.get(2)?,
        display_name: row.get(3)?,
        enabled: row.get(4)?,
        status: RootStatus::from_str(&status).map_err(to_sql_conversion_error)?,
    })
}

type RawStageState = (
    i64,
    Option<i64>,
    Option<i64>,
    String,
    String,
    String,
    Option<String>,
    i64,
);

fn stage_state_from_row(row: &Row<'_>) -> rusqlite::Result<RawStageState> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}

fn stage_state_from_raw(raw: RawStageState) -> Result<StageState> {
    Ok(StageState {
        id: raw.0,
        target: parse_target(raw.1, raw.2)?,
        stage: Stage::from_str(&raw.3)?,
        status: StageStatus::from_str(&raw.4)?,
        version: raw.5,
        error: raw.6,
        updated_at: raw.7,
    })
}

fn target_ids(target: StageTarget) -> (Option<i64>, Option<i64>) {
    match target {
        StageTarget::Asset(id) => (Some(id), None),
        StageTarget::Location(id) => (None, Some(id)),
    }
}

fn target_parts(target: StageTarget) -> (&'static str, i64) {
    match target {
        StageTarget::Asset(id) => ("asset_id", id),
        StageTarget::Location(id) => ("location_id", id),
    }
}

fn parse_target(asset_id: Option<i64>, location_id: Option<i64>) -> Result<StageTarget> {
    match (asset_id, location_id) {
        (Some(id), None) => Ok(StageTarget::Asset(id)),
        (None, Some(id)) => Ok(StageTarget::Location(id)),
        value => Err(StoreError::CorruptData {
            field: "stage target",
            value: format!("{value:?}"),
        }),
    }
}

fn validate_nonempty(field: &'static str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(StoreError::InvalidInput {
            field,
            reason: "must not be empty".to_owned(),
        });
    }
    Ok(())
}

fn validate_absolute_path(value: &str) -> Result<()> {
    validate_nonempty("absolute_path", value)?;
    if !std::path::Path::new(value).is_absolute() {
        return Err(StoreError::InvalidInput {
            field: "absolute_path",
            reason: "must be absolute".to_owned(),
        });
    }
    Ok(())
}

fn validate_relative_path(value: &str) -> Result<()> {
    validate_nonempty("relative_path", value)?;
    if std::path::Path::new(value).components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(StoreError::InvalidInput {
            field: "relative_path",
            reason: "must remain below its root".to_owned(),
        });
    }
    Ok(())
}

fn fixed_blob<const N: usize>(field: &'static str, value: Vec<u8>) -> Result<[u8; N]> {
    let length = value.len();
    value.try_into().map_err(|_| StoreError::CorruptData {
        field,
        value: format!("blob length {length}"),
    })
}

fn corrupt(field: &'static str, value: impl ToString) -> StoreError {
    StoreError::CorruptData {
        field,
        value: value.to_string(),
    }
}

fn to_sql_conversion_error(error: StoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn collect<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>> {
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(StoreError::from)
}
