use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{Album, AlbumItem, AlbumKind, AlbumWithCount, Result, StoreError};

fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(StoreError::InvalidInput {
            field: "album name",
            reason: "must not be empty".to_owned(),
        });
    }
    Ok(())
}

fn validate_query(query: &str) -> Result<()> {
    if query.trim().is_empty() || query.len() > 64 * 1024 {
        return Err(StoreError::InvalidInput {
            field: "album query",
            reason: "must be non-empty and at most 64 KiB".to_owned(),
        });
    }
    Ok(())
}

fn album_from_row(row: &Row<'_>) -> rusqlite::Result<Album> {
    let kind: String = row.get(2)?;
    Ok(Album {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: kind.parse().map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        query: row.get(3)?,
        sort_order: row.get(4)?,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct AlbumRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> AlbumRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn create(&self, name: &str, sort_order: i64) -> Result<Album> {
        validate_name(name)?;
        self.connection.execute(
            "INSERT INTO albums (name, kind, query, sort_order) VALUES (?1, 'manual', NULL, ?2)",
            params![name.trim(), sort_order],
        )?;
        self.get(self.connection.last_insert_rowid())
    }

    /// Creates an automatic album whose membership is evaluated from `query`.
    pub fn create_auto(&self, name: &str, query: &str, sort_order: i64) -> Result<Album> {
        validate_name(name)?;
        validate_query(query)?;
        self.connection.execute(
            "INSERT INTO albums (name, kind, query, sort_order) VALUES (?1, 'auto', ?2, ?3)",
            params![name.trim(), query, sort_order],
        )?;
        self.get(self.connection.last_insert_rowid())
    }

    /// Replaces the saved condition of an automatic album.
    pub fn set_query(&self, id: i64, query: &str) -> Result<Album> {
        validate_query(query)?;
        if self.get(id)?.kind != AlbumKind::Auto {
            return Err(StoreError::InvalidInput {
                field: "album kind",
                reason: "only automatic albums have a condition".to_owned(),
            });
        }
        self.connection.execute(
            "UPDATE albums SET query = ?1, updated_at = unixepoch() WHERE id = ?2",
            params![query, id],
        )?;
        self.get(id)
    }

    /// Returns the first automatic album with this name, if any.
    pub fn find_auto_by_name(&self, name: &str) -> Result<Option<Album>> {
        Ok(self
            .connection
            .query_row(
                "SELECT id, name, kind, query, sort_order FROM albums
                 WHERE kind = 'auto' AND name = ?1 ORDER BY id LIMIT 1",
                [name.trim()],
                album_from_row,
            )
            .optional()?)
    }

    /// Returns the first manual album with this name, if any.
    pub fn find_manual_by_name(&self, name: &str) -> Result<Option<Album>> {
        Ok(self
            .connection
            .query_row(
                "SELECT id, name, kind, query, sort_order FROM albums
                 WHERE kind = 'manual' AND name = ?1 ORDER BY id LIMIT 1",
                [name.trim()],
                album_from_row,
            )
            .optional()?)
    }

    pub fn get(&self, id: i64) -> Result<Album> {
        Ok(self.connection.query_row(
            "SELECT id, name, kind, query, sort_order FROM albums WHERE id = ?1",
            [id],
            album_from_row,
        )?)
    }

    pub fn list(&self) -> Result<Vec<AlbumWithCount>> {
        let mut statement = self.connection.prepare(
            "SELECT albums.id, albums.name, albums.kind, albums.query, albums.sort_order,
                    (SELECT COUNT(*) FROM album_items WHERE album_items.album_id = albums.id)
             FROM albums ORDER BY albums.sort_order, albums.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(AlbumWithCount {
                album: album_from_row(row)?,
                asset_count: row.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    pub fn rename(&self, id: i64, name: &str) -> Result<Album> {
        validate_name(name)?;
        let changed = self.connection.execute(
            "UPDATE albums SET name = ?1, updated_at = unixepoch() WHERE id = ?2",
            params![name.trim(), id],
        )?;
        if changed == 0 {
            return Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        }
        self.get(id)
    }

    pub fn delete(&self, id: i64) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM albums WHERE id = ?1", [id])?
            == 1)
    }

    /// Appends assets to a manual album, preserving existing order and ignoring duplicates.
    pub fn add_assets(&self, album_id: i64, asset_ids: &[i64]) -> Result<usize> {
        self.ensure_manual(album_id)?;
        let transaction = self.connection.unchecked_transaction()?;
        let start: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM album_items WHERE album_id = ?1",
            [album_id],
            |row| row.get(0),
        )?;
        let mut added = 0;
        for (offset, asset_id) in asset_ids.iter().enumerate() {
            added += transaction.execute(
                "INSERT INTO album_items (album_id, asset_id, position) VALUES (?1, ?2, ?3)
                 ON CONFLICT (album_id, asset_id) DO NOTHING",
                params![album_id, asset_id, start + offset as i64],
            )?;
        }
        transaction.commit()?;
        Ok(added)
    }

    /// Inserts one item at an explicit position, keeping an existing item as is.
    /// Used to restore exported order when assets arrive in any order.
    pub fn insert_at(&self, album_id: i64, asset_id: i64, position: i64) -> Result<bool> {
        self.ensure_manual(album_id)?;
        if position < 0 {
            return Err(StoreError::InvalidInput {
                field: "album position",
                reason: "must not be negative".to_owned(),
            });
        }
        let inserted = self.connection.execute(
            "INSERT INTO album_items (album_id, asset_id, position) VALUES (?1, ?2, ?3)
             ON CONFLICT (album_id, asset_id) DO NOTHING",
            params![album_id, asset_id, position],
        )?;
        Ok(inserted > 0)
    }

    pub fn remove_assets(&self, album_id: i64, asset_ids: &[i64]) -> Result<usize> {
        self.ensure_manual(album_id)?;
        let transaction = self.connection.unchecked_transaction()?;
        let mut removed = 0;
        for asset_id in asset_ids {
            removed += transaction.execute(
                "DELETE FROM album_items WHERE album_id = ?1 AND asset_id = ?2",
                params![album_id, asset_id],
            )?;
        }
        compact(&transaction, album_id)?;
        transaction.commit()?;
        Ok(removed)
    }

    pub fn list_items(&self, album_id: i64) -> Result<Vec<AlbumItem>> {
        self.get(album_id)?;
        let mut statement = self.connection.prepare(
            "SELECT album_id, asset_id, position FROM album_items
             WHERE album_id = ?1 ORDER BY position, asset_id",
        )?;
        let rows = statement.query_map([album_id], |row| {
            Ok(AlbumItem {
                album_id: row.get(0)?,
                asset_id: row.get(1)?,
                position: row.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    /// Reorders items. Unknown or duplicate ids are rejected before any write.
    pub fn reorder(&self, album_id: i64, asset_ids: &[i64]) -> Result<()> {
        self.ensure_manual(album_id)?;
        let existing = self
            .list_items(album_id)?
            .into_iter()
            .map(|item| item.asset_id)
            .collect::<Vec<_>>();
        let mut requested = asset_ids.to_vec();
        requested.sort_unstable();
        requested.dedup();
        let mut current = existing.clone();
        current.sort_unstable();
        if requested != current || requested.len() != asset_ids.len() {
            return Err(StoreError::InvalidInput {
                field: "album items",
                reason: "reorder must list every album asset exactly once".to_owned(),
            });
        }
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "UPDATE album_items SET position = position + 1000000 WHERE album_id = ?1",
            [album_id],
        )?;
        for (position, asset_id) in asset_ids.iter().enumerate() {
            transaction.execute(
                "UPDATE album_items SET position = ?1 WHERE album_id = ?2 AND asset_id = ?3",
                params![position as i64, album_id, asset_id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn ensure_manual(&self, album_id: i64) -> Result<Album> {
        let album = self.get(album_id)?;
        if album.kind != AlbumKind::Manual {
            return Err(StoreError::InvalidInput {
                field: "album kind",
                reason: "automatic albums cannot be edited".to_owned(),
            });
        }
        Ok(album)
    }
}

fn compact(connection: &Connection, album_id: i64) -> Result<()> {
    let ids = {
        let mut statement = connection.prepare(
            "SELECT asset_id FROM album_items WHERE album_id = ?1 ORDER BY position, asset_id",
        )?;
        let rows = statement.query_map([album_id], |row| row.get::<_, i64>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (position, asset_id) in ids.iter().enumerate() {
        connection.execute(
            "UPDATE album_items SET position = ?1 WHERE album_id = ?2 AND asset_id = ?3",
            params![position as i64, album_id, asset_id],
        )?;
    }
    Ok(())
}

/// Returns whether an album item exists; used by focused repository tests.
pub fn item_exists(connection: &Connection, album_id: i64, asset_id: i64) -> Result<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM album_items WHERE album_id = ?1 AND asset_id = ?2",
            params![album_id, asset_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}
