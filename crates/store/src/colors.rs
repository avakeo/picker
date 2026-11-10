use rusqlite::{params, Connection};

use crate::{AssetColor, Result, StoreError};

pub fn validate_hex(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_colors(colors: &[(String, f64)]) -> Result<()> {
    if colors.len() > 3 {
        return Err(StoreError::InvalidInput {
            field: "colors",
            reason: "at most three colors are allowed".to_owned(),
        });
    }
    for (hex, ratio) in colors {
        if !validate_hex(hex) {
            return Err(StoreError::InvalidInput {
                field: "hex",
                reason: "must be #RRGGBB".to_owned(),
            });
        }
        if !(0.0..=1.0).contains(ratio) || !ratio.is_finite() {
            return Err(StoreError::InvalidInput {
                field: "ratio",
                reason: "must be between 0 and 1".to_owned(),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct ColorRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> ColorRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn list(&self, asset_id: i64) -> Result<Vec<AssetColor>> {
        let mut statement = self.connection.prepare(
            "SELECT asset_id, rank, hex, ratio, source, algorithm_version
             FROM asset_colors WHERE asset_id = ?1 ORDER BY rank",
        )?;
        let rows = statement.query_map([asset_id], |row| {
            let source: String = row.get(4)?;
            Ok(AssetColor {
                asset_id: row.get(0)?,
                rank: row.get(1)?,
                hex: row.get(2)?,
                ratio: row.get(3)?,
                source: source.parse().map_err(to_sql)?,
                algorithm_version: row.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    /// Returns `(asset_id, hex)` for every color ranked `max_rank` or better,
    /// for color search. Rank 1 is the main color.
    pub fn colors_up_to_rank(&self, max_rank: u8) -> Result<Vec<(i64, String)>> {
        let mut statement = self.connection.prepare(
            "SELECT asset_id, hex FROM asset_colors WHERE rank <= ?1 ORDER BY asset_id, rank",
        )?;
        let rows = statement.query_map([max_rank], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    /// Returns whether the user has edited this asset's colors.
    /// Returns the ids of assets whose colors the user has set.
    pub fn manual_assets(&self) -> Result<Vec<i64>> {
        let mut statement = self
            .connection
            .prepare("SELECT asset_id FROM asset_color_manual ORDER BY asset_id")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    pub fn is_manual(&self, asset_id: i64) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM asset_color_manual WHERE asset_id = ?1)",
            [asset_id],
            |row| row.get(0),
        )?)
    }

    /// Writes automatic colors unless the user has taken ownership.
    ///
    /// Returns `false` when the asset is manually managed. Automatic reruns
    /// never delete or replace user colors.
    pub fn set_auto(
        &self,
        asset_id: i64,
        colors: &[(String, f64)],
        algorithm_version: &str,
    ) -> Result<bool> {
        validate_colors(colors)?;
        if algorithm_version.trim().is_empty() {
            return Err(StoreError::InvalidInput {
                field: "algorithm_version",
                reason: "must not be empty".to_owned(),
            });
        }
        if self.is_manual(asset_id)? {
            return Ok(false);
        }
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute("DELETE FROM asset_colors WHERE asset_id = ?1", [asset_id])?;
        for (index, (hex, ratio)) in colors.iter().enumerate() {
            transaction.execute(
                "INSERT INTO asset_colors
                    (asset_id, rank, hex, ratio, source, algorithm_version)
                 VALUES (?1, ?2, ?3, ?4, 'auto', ?5)",
                params![asset_id, index as i64 + 1, hex, ratio, algorithm_version],
            )?;
        }
        transaction.commit()?;
        Ok(true)
    }

    /// Replaces the asset's colors with user-owned values, including an empty list.
    pub fn set_user(&self, asset_id: i64, colors: &[(String, f64)]) -> Result<Vec<AssetColor>> {
        validate_colors(colors)?;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute("DELETE FROM asset_colors WHERE asset_id = ?1", [asset_id])?;
        for (index, (hex, ratio)) in colors.iter().enumerate() {
            transaction.execute(
                "INSERT INTO asset_colors
                    (asset_id, rank, hex, ratio, source, algorithm_version)
                 VALUES (?1, ?2, ?3, ?4, 'user', 'manual')",
                params![asset_id, index as i64 + 1, hex, ratio],
            )?;
        }
        transaction.execute(
            "INSERT INTO asset_color_manual (asset_id) VALUES (?1)
             ON CONFLICT (asset_id) DO UPDATE SET updated_at = unixepoch()",
            [asset_id],
        )?;
        transaction.commit()?;
        self.list(asset_id)
    }

    /// Drops the manual override so the next color stage can repopulate colors.
    pub fn clear_manual(&self, asset_id: i64) -> Result<bool> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM asset_colors WHERE asset_id = ?1 AND source = 'user'",
            [asset_id],
        )?;
        let removed = transaction.execute(
            "DELETE FROM asset_color_manual WHERE asset_id = ?1",
            [asset_id],
        )?;
        transaction.commit()?;
        Ok(removed == 1)
    }
}

fn to_sql(error: StoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}
