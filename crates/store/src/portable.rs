use rusqlite::{params, Connection, OptionalExtension};

use crate::{AssignmentSource, PendingItemKind, Result, StoreError, TagState};

/// One decided assignment keyed by the asset content hash.
#[derive(Debug, Clone, PartialEq)]
pub struct PortableAssignment {
    pub blake3_hash: Vec<u8>,
    pub namespace: String,
    pub canonical_name: String,
    pub state: TagState,
    pub source: AssignmentSource,
    pub confidence: Option<f64>,
    pub model_id: Option<String>,
    pub model_version: Option<String>,
}

/// Imported data other than tag assignments, waiting for its asset to be scanned.
/// The payload is the exported JSON record, kept as is so a re-export can copy it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingItem {
    pub blake3_hash: Vec<u8>,
    pub kind: PendingItemKind,
    pub payload: String,
}

#[derive(Debug, Clone, Copy)]
pub struct PortableRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> PortableRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    /// Returns confirmed and rejected assignments, including staged ones
    /// whose asset has not been scanned yet, so a re-export keeps them.
    pub fn decided_assignments(&self) -> Result<Vec<PortableAssignment>> {
        let mut statement = self.connection.prepare(
            "SELECT assets.blake3_hash, tags.namespace, tags.canonical_name,
                    assignments.state, assignments.source, assignments.confidence,
                    assignments.model_id, assignments.model_version
             FROM tag_assignments assignments
             JOIN assets ON assets.id = assignments.asset_id
             JOIN tags ON tags.id = assignments.tag_id
             WHERE assignments.state IN ('confirmed', 'rejected')
             UNION ALL
             SELECT pending.blake3_hash, tags.namespace, tags.canonical_name,
                    pending.state, pending.source, pending.confidence,
                    pending.model_id, pending.model_version
             FROM pending_tag_assignments pending
             JOIN tags ON tags.id = pending.tag_id
             WHERE pending.state IN ('confirmed', 'rejected')
               AND NOT EXISTS (
                   SELECT 1 FROM tag_assignments assigned
                   JOIN assets ON assets.id = assigned.asset_id
                   WHERE assets.blake3_hash = pending.blake3_hash
                     AND assigned.tag_id = pending.tag_id
               )
             ORDER BY 1, 2, 3",
        )?;
        let rows = statement.query_map([], |row| {
            let state: String = row.get(3)?;
            let source: String = row.get(4)?;
            Ok(PortableAssignment {
                blake3_hash: row.get(0)?,
                namespace: row.get(1)?,
                canonical_name: row.get(2)?,
                state: state.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                source: source.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                confidence: row.get(5)?,
                model_id: row.get(6)?,
                model_version: row.get(7)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stage_assignment(
        &self,
        blake3_hash: &[u8],
        tag_id: i64,
        state: TagState,
        source: AssignmentSource,
        confidence: Option<f64>,
        model_id: Option<&str>,
        model_version: Option<&str>,
    ) -> Result<()> {
        if blake3_hash.len() != 32 {
            return Err(StoreError::InvalidInput {
                field: "blake3_hash",
                reason: "must be 32 bytes".to_owned(),
            });
        }
        self.connection.execute(
            "INSERT INTO pending_tag_assignments
                (blake3_hash, tag_id, state, source, confidence, model_id, model_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (blake3_hash, tag_id) DO UPDATE SET
                 state = excluded.state,
                 source = excluded.source,
                 confidence = excluded.confidence,
                 model_id = excluded.model_id,
                 model_version = excluded.model_version",
            params![
                blake3_hash,
                tag_id,
                state.as_str(),
                source.as_str(),
                confidence,
                model_id,
                model_version
            ],
        )?;
        Ok(())
    }

    /// Applies pending assignments for one asset and removes them.
    ///
    /// An imported decision replaces a machine suggestion, but never an
    /// existing confirmation or rejection.
    pub fn apply_for_asset(&self, asset_id: i64, blake3_hash: &[u8]) -> Result<usize> {
        let transaction = self.connection.unchecked_transaction()?;
        let changed = transaction.execute(
            "INSERT INTO tag_assignments
                (asset_id, tag_id, state, source, confidence, model_id, model_version)
             SELECT ?1, tag_id, state, source, confidence, model_id, model_version
             FROM pending_tag_assignments WHERE blake3_hash = ?2
             ON CONFLICT (asset_id, tag_id) DO UPDATE SET
                 state = excluded.state,
                 source = excluded.source,
                 confidence = excluded.confidence,
                 model_id = excluded.model_id,
                 model_version = excluded.model_version,
                 updated_at = unixepoch()
             WHERE tag_assignments.state = 'suggested'
               AND excluded.state IN ('confirmed', 'rejected')",
            params![asset_id, blake3_hash],
        )?;
        transaction.execute(
            "DELETE FROM pending_tag_assignments WHERE blake3_hash = ?1",
            [blake3_hash],
        )?;
        transaction.commit()?;
        Ok(changed)
    }

    /// Applies every pending assignment whose asset already exists, with the
    /// same precedence as [`Self::apply_for_asset`].
    pub fn apply_all(&self) -> Result<usize> {
        let transaction = self.connection.unchecked_transaction()?;
        let changed = transaction.execute(
            "INSERT INTO tag_assignments
                (asset_id, tag_id, state, source, confidence, model_id, model_version)
             SELECT assets.id, pending.tag_id, pending.state, pending.source,
                    pending.confidence, pending.model_id, pending.model_version
             FROM pending_tag_assignments pending
             JOIN assets ON assets.blake3_hash = pending.blake3_hash
             WHERE true
             ON CONFLICT (asset_id, tag_id) DO UPDATE SET
                 state = excluded.state,
                 source = excluded.source,
                 confidence = excluded.confidence,
                 model_id = excluded.model_id,
                 model_version = excluded.model_version,
                 updated_at = unixepoch()
             WHERE tag_assignments.state = 'suggested'
               AND excluded.state IN ('confirmed', 'rejected')",
            [],
        )?;
        transaction.execute(
            "DELETE FROM pending_tag_assignments
             WHERE blake3_hash IN (SELECT blake3_hash FROM assets)",
            [],
        )?;
        transaction.commit()?;
        Ok(changed)
    }

    pub fn pending_count(&self) -> Result<u32> {
        self.connection
            .query_row("SELECT COUNT(*) FROM pending_tag_assignments", [], |row| {
                row.get(0)
            })
            .optional()?
            .ok_or_else(|| StoreError::CorruptData {
                field: "pending count",
                value: "missing".to_owned(),
            })
    }

    pub fn stage_item(
        &self,
        blake3_hash: &[u8],
        kind: PendingItemKind,
        payload: &str,
    ) -> Result<()> {
        if blake3_hash.len() != 32 {
            return Err(StoreError::InvalidInput {
                field: "blake3_hash",
                reason: "must be 32 bytes".to_owned(),
            });
        }
        self.connection.execute(
            "INSERT INTO pending_portable_items (blake3_hash, kind, payload) VALUES (?1, ?2, ?3)",
            params![blake3_hash, kind.as_str(), payload],
        )?;
        Ok(())
    }

    /// Returns every pending item in insertion order.
    pub fn pending_items(&self) -> Result<Vec<PendingItem>> {
        self.items_where("1 = 1", [])
    }

    /// Removes and returns the pending items for one asset hash, in insertion order.
    pub fn take_items(&self, blake3_hash: &[u8]) -> Result<Vec<PendingItem>> {
        let items = self.items_where("blake3_hash = ?1", [blake3_hash])?;
        self.connection.execute(
            "DELETE FROM pending_portable_items WHERE blake3_hash = ?1",
            [blake3_hash],
        )?;
        Ok(items)
    }

    fn items_where<P: rusqlite::Params>(
        &self,
        condition: &str,
        values: P,
    ) -> Result<Vec<PendingItem>> {
        let sql = format!(
            "SELECT blake3_hash, kind, payload FROM pending_portable_items
             WHERE {condition} ORDER BY id"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(values, |row| {
            let kind: String = row.get(1)?;
            Ok(PendingItem {
                blake3_hash: row.get(0)?,
                kind: kind.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                payload: row.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }
}
