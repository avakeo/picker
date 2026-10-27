use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{CustomClassifier, Result, StoreError};

#[derive(Debug, Clone, Copy)]
pub struct ClassifierRepository<'connection> {
    connection: &'connection Connection,
}

fn decode(row: &Row<'_>) -> rusqlite::Result<CustomClassifier> {
    let bytes: Vec<u8> = row.get(2)?;
    let weights = bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|chunk| f64::from_le_bytes(*chunk))
        .collect();
    Ok(CustomClassifier {
        tag_id: row.get(0)?,
        dimensions: row.get(1)?,
        weights,
        bias: row.get(3)?,
        trained_at: row.get(4)?,
    })
}

impl<'connection> ClassifierRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn upsert(
        &self,
        tag_id: i64,
        dimensions: u32,
        weights: &[f64],
        bias: f64,
    ) -> Result<CustomClassifier> {
        if dimensions == 0 || weights.len() != dimensions as usize {
            return Err(StoreError::InvalidInput {
                field: "weights",
                reason: "length must match positive dimensions".to_owned(),
            });
        }
        let bytes: Vec<u8> = weights
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        self.connection.execute(
            "INSERT INTO custom_classifiers (tag_id, dimensions, weights, bias)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (tag_id) DO UPDATE SET
                 dimensions = excluded.dimensions,
                 weights = excluded.weights,
                 bias = excluded.bias,
                 trained_at = unixepoch()",
            params![tag_id, dimensions, bytes, bias],
        )?;
        self.get(tag_id)?.ok_or_else(|| StoreError::CorruptData {
            field: "custom_classifier",
            value: "upserted row was not found".to_owned(),
        })
    }

    pub fn get(&self, tag_id: i64) -> Result<Option<CustomClassifier>> {
        self.connection
            .query_row(
                "SELECT tag_id, dimensions, weights, bias, trained_at
                 FROM custom_classifiers WHERE tag_id = ?1",
                [tag_id],
                decode,
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn delete(&self, tag_id: i64) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM custom_classifiers WHERE tag_id = ?1", [tag_id])?
            == 1)
    }
}
