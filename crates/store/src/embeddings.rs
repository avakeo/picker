use rusqlite::{params, Connection};

use crate::{Embedding, Result, StoreError};

const MAX_DIMENSIONS: usize = 4096;

#[derive(Debug, Clone, Copy)]
pub struct EmbeddingRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> EmbeddingRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn upsert(
        &self,
        asset_id: i64,
        model_id: &str,
        model_version: &str,
        vector: &[f32],
    ) -> Result<()> {
        validate(model_id, model_version, vector)?;
        let bytes = vector
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>();
        self.connection.execute(
            "INSERT INTO embeddings
                (asset_id, model_id, model_version, dimensions, vector, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, unixepoch())
             ON CONFLICT (asset_id, model_id, model_version) DO UPDATE SET
                dimensions = excluded.dimensions,
                vector = excluded.vector,
                updated_at = excluded.updated_at",
            params![
                asset_id,
                model_id,
                model_version,
                i64::try_from(vector.len()).unwrap_or(i64::MAX),
                bytes
            ],
        )?;
        Ok(())
    }

    pub fn get(
        &self,
        asset_id: i64,
        model_id: &str,
        model_version: &str,
    ) -> Result<Option<Embedding>> {
        let mut statement = self.connection.prepare(
            "SELECT asset_id, model_id, model_version, dimensions, vector, updated_at
             FROM embeddings
             WHERE asset_id = ?1 AND model_id = ?2 AND model_version = ?3",
        )?;
        let mut rows = statement.query(params![asset_id, model_id, model_version])?;
        match rows.next()? {
            Some(row) => Ok(Some(decode(row)?)),
            None => Ok(None),
        }
    }

    pub fn list_for_model(&self, model_id: &str, model_version: &str) -> Result<Vec<Embedding>> {
        let mut statement = self.connection.prepare(
            "SELECT asset_id, model_id, model_version, dimensions, vector, updated_at
             FROM embeddings
             WHERE model_id = ?1 AND model_version = ?2
             ORDER BY asset_id",
        )?;
        let rows = statement.query_map(params![model_id, model_version], decode)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    pub fn count_for_model(&self, model_id: &str, model_version: &str) -> Result<u32> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*) FROM embeddings WHERE model_id = ?1 AND model_version = ?2",
            params![model_id, model_version],
            |row| row.get(0),
        )?)
    }

    /// Returns assets that still need an embedding, newest id first for stable batching.
    pub fn missing_asset_ids(
        &self,
        model_id: &str,
        model_version: &str,
        limit: u32,
    ) -> Result<Vec<i64>> {
        let mut statement = self.connection.prepare(
            "SELECT a.id FROM assets a
             WHERE NOT EXISTS (
                SELECT 1 FROM embeddings e
                WHERE e.asset_id = a.id AND e.model_id = ?1 AND e.model_version = ?2
             )
             ORDER BY a.id LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![model_id, model_version, i64::from(limit.clamp(1, 1000))],
            |row| row.get(0),
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    pub fn count_missing(&self, model_id: &str, model_version: &str) -> Result<u32> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*) FROM assets a
             WHERE NOT EXISTS (
                SELECT 1 FROM embeddings e
                WHERE e.asset_id = a.id AND e.model_id = ?1 AND e.model_version = ?2
             )",
            params![model_id, model_version],
            |row| row.get(0),
        )?)
    }

    pub fn delete(&self, asset_id: i64, model_id: &str, model_version: &str) -> Result<bool> {
        Ok(self.connection.execute(
            "DELETE FROM embeddings
             WHERE asset_id = ?1 AND model_id = ?2 AND model_version = ?3",
            params![asset_id, model_id, model_version],
        )? > 0)
    }
}

fn validate(model_id: &str, model_version: &str, vector: &[f32]) -> Result<()> {
    if model_id.trim().is_empty() || model_version.trim().is_empty() {
        return Err(StoreError::InvalidInput {
            field: "model",
            reason: "model id and version must not be empty".to_owned(),
        });
    }
    if vector.is_empty() || vector.len() > MAX_DIMENSIONS {
        return Err(StoreError::InvalidInput {
            field: "vector",
            reason: format!("dimension must be between 1 and {MAX_DIMENSIONS}"),
        });
    }
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(StoreError::InvalidInput {
            field: "vector",
            reason: "values must be finite".to_owned(),
        });
    }
    Ok(())
}

fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Embedding> {
    let dimensions: u32 = row.get(3)?;
    let bytes: Vec<u8> = row.get(4)?;
    if bytes.len() != dimensions as usize * 4 {
        return Err(rusqlite::Error::InvalidColumnType(
            4,
            "vector".to_owned(),
            rusqlite::types::Type::Blob,
        ));
    }
    let vector = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect();
    Ok(Embedding {
        asset_id: row.get(0)?,
        model_id: row.get(1)?,
        model_version: row.get(2)?,
        dimensions,
        vector,
        updated_at: row.get(5)?,
    })
}
