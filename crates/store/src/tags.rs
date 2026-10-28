use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension, Row};

use crate::{
    Asset, AssetFilter, AssignmentSource, NewTag, Result, StoreError, Tag, TagAlias, TagAssignment,
    TagState, TagWithCount,
};

fn validate_nonempty(field: &'static str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(StoreError::InvalidInput {
            field,
            reason: "must not be empty".to_owned(),
        });
    }
    Ok(())
}

fn to_sql(error: StoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn tag_from_row(row: &Row<'_>) -> rusqlite::Result<Tag> {
    let origin: String = row.get(7)?;
    Ok(Tag {
        id: row.get(0)?,
        namespace: row.get(1)?,
        canonical_name: row.get(2)?,
        display_name_ja: row.get(3)?,
        display_name_en: row.get(4)?,
        parent_id: row.get(5)?,
        color: row.get(6)?,
        origin: origin.parse().map_err(to_sql)?,
    })
}

fn assignment_from_row(row: &Row<'_>) -> rusqlite::Result<TagAssignment> {
    let state: String = row.get(2)?;
    let source: String = row.get(3)?;
    Ok(TagAssignment {
        asset_id: row.get(0)?,
        tag_id: row.get(1)?,
        state: state.parse().map_err(to_sql)?,
        source: source.parse().map_err(to_sql)?,
        confidence: row.get(4)?,
        model_id: row.get(5)?,
        model_version: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct TagRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> TagRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn find(&self, namespace: &str, canonical_name: &str) -> Result<Option<Tag>> {
        self.connection
            .query_row(
                "SELECT id, namespace, canonical_name, display_name_ja, display_name_en,
                        parent_id, color, origin
                 FROM tags WHERE namespace = ?1 AND canonical_name = ?2",
                params![namespace, canonical_name],
                tag_from_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn create(&self, tag: &NewTag<'_>) -> Result<Tag> {
        validate_nonempty("canonical_name", tag.canonical_name)?;
        validate_nonempty("display_name_ja", tag.display_name_ja)?;
        validate_nonempty("namespace", tag.namespace)?;
        if let Some(color) = tag.color {
            if !valid_color(color) {
                return Err(StoreError::InvalidInput {
                    field: "color",
                    reason: "must be #RRGGBB".to_owned(),
                });
            }
        }
        if let Some(parent_id) = tag.parent_id {
            self.get(parent_id)?;
        }
        self.connection.execute(
            "INSERT INTO tags
                (namespace, canonical_name, display_name_ja, display_name_en, parent_id, color, origin)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                tag.namespace.trim(),
                tag.canonical_name.trim(),
                tag.display_name_ja.trim(),
                tag.display_name_en.map(str::trim),
                tag.parent_id,
                tag.color,
                tag.origin.as_str()
            ],
        )?;
        self.get(self.connection.last_insert_rowid())
    }

    pub fn get(&self, id: i64) -> Result<Tag> {
        Ok(self.connection.query_row(
            "SELECT id, namespace, canonical_name, display_name_ja, display_name_en,
                    parent_id, color, origin
             FROM tags WHERE id = ?1",
            [id],
            tag_from_row,
        )?)
    }

    pub fn find_by_name(&self, namespace: &str, canonical_name: &str) -> Result<Option<Tag>> {
        Ok(self
            .connection
            .query_row(
                "SELECT id, namespace, canonical_name, display_name_ja, display_name_en,
                        parent_id, color, origin
                 FROM tags WHERE namespace = ?1 AND canonical_name = ?2",
                params![namespace, canonical_name.trim()],
                tag_from_row,
            )
            .optional()?)
    }

    pub fn find_by_alias(&self, alias: &str) -> Result<Option<Tag>> {
        Ok(self
            .connection
            .query_row(
                "SELECT tags.id, tags.namespace, tags.canonical_name, tags.display_name_ja,
                        tags.display_name_en, tags.parent_id, tags.color, tags.origin
                 FROM tags JOIN tag_aliases ON tag_aliases.tag_id = tags.id
                 WHERE tag_aliases.alias = ?1",
                [alias.trim()],
                tag_from_row,
            )
            .optional()?)
    }

    pub fn list(&self) -> Result<Vec<Tag>> {
        let mut statement = self.connection.prepare(
            "SELECT id, namespace, canonical_name, display_name_ja, display_name_en,
                    parent_id, color, origin
             FROM tags ORDER BY namespace, canonical_name",
        )?;
        let rows = statement.query_map([], tag_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    /// Asset ids whose tag name, alias, or display name contains `query`.
    pub fn asset_ids_matching(&self, query: &str) -> Result<Vec<i64>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT assignments.asset_id
             FROM tag_assignments assignments
             JOIN tags ON tags.id = assignments.tag_id
             LEFT JOIN tag_aliases aliases ON aliases.tag_id = tags.id
             WHERE assignments.state IN ('suggested', 'confirmed')
               AND (
                 instr(lower(tags.canonical_name), lower(?1)) > 0
                 OR instr(lower(tags.display_name_ja), lower(?1)) > 0
                 OR instr(lower(coalesce(tags.display_name_en, '')), lower(?1)) > 0
                 OR instr(lower(coalesce(aliases.alias, '')), lower(?1)) > 0
               )
             ORDER BY assignments.asset_id",
        )?;
        let rows = statement.query_map([query.trim()], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    pub fn list_with_counts(&self) -> Result<Vec<TagWithCount>> {
        let mut statement = self.connection.prepare(
            "SELECT tags.id, tags.namespace, tags.canonical_name, tags.display_name_ja,
                    tags.display_name_en, tags.parent_id, tags.color, tags.origin,
                    (SELECT COUNT(*) FROM tag_assignments
                     WHERE tag_assignments.tag_id = tags.id
                       AND tag_assignments.state = 'confirmed')
             FROM tags ORDER BY tags.namespace, tags.canonical_name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(TagWithCount {
                tag: tag_from_row(row)?,
                asset_count: row.get(8)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    pub fn convert_to_custom(&self, id: i64) -> Result<Tag> {
        let changed = self.connection.execute(
            "UPDATE tags SET namespace = 'custom', origin = 'user', updated_at = unixepoch()
             WHERE id = ?1",
            [id],
        )?;
        if changed == 0 {
            return Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        }
        self.get(id)
    }

    pub fn rename(&self, id: i64, canonical_name: &str, display_name_ja: &str) -> Result<Tag> {
        validate_nonempty("canonical_name", canonical_name)?;
        validate_nonempty("display_name_ja", display_name_ja)?;
        let changed = self.connection.execute(
            "UPDATE tags SET canonical_name = ?1, display_name_ja = ?2, updated_at = unixepoch()
             WHERE id = ?3",
            params![canonical_name.trim(), display_name_ja.trim(), id],
        )?;
        if changed == 0 {
            return Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        }
        self.get(id)
    }

    pub fn set_parent(&self, id: i64, parent_id: Option<i64>) -> Result<Tag> {
        self.get(id)?;
        if let Some(parent_id) = parent_id {
            if parent_id == id {
                return Err(StoreError::InvalidInput {
                    field: "parent_id",
                    reason: "a tag cannot be its own parent".to_owned(),
                });
            }
            self.get(parent_id)?;
            if self.descendants(id)?.contains(&parent_id) {
                return Err(StoreError::InvalidInput {
                    field: "parent_id",
                    reason: "would create a tag hierarchy cycle".to_owned(),
                });
            }
        }
        self.connection.execute(
            "UPDATE tags SET parent_id = ?1, updated_at = unixepoch() WHERE id = ?2",
            params![parent_id, id],
        )?;
        self.get(id)
    }

    pub fn delete(&self, id: i64) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM tags WHERE id = ?1", [id])?
            == 1)
    }

    pub fn add_alias(&self, tag_id: i64, alias: &str) -> Result<TagAlias> {
        validate_nonempty("alias", alias)?;
        self.get(tag_id)?;
        self.connection.execute(
            "INSERT INTO tag_aliases (tag_id, alias) VALUES (?1, ?2)",
            params![tag_id, alias.trim()],
        )?;
        Ok(self.connection.query_row(
            "SELECT id, tag_id, alias FROM tag_aliases WHERE id = ?1",
            [self.connection.last_insert_rowid()],
            |row| {
                Ok(TagAlias {
                    id: row.get(0)?,
                    tag_id: row.get(1)?,
                    alias: row.get(2)?,
                })
            },
        )?)
    }

    pub fn remove_alias(&self, id: i64) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM tag_aliases WHERE id = ?1", [id])?
            == 1)
    }

    pub fn aliases(&self, tag_id: i64) -> Result<Vec<TagAlias>> {
        let mut statement = self.connection.prepare(
            "SELECT id, tag_id, alias FROM tag_aliases WHERE tag_id = ?1 ORDER BY alias",
        )?;
        let rows = statement.query_map([tag_id], |row| {
            Ok(TagAlias {
                id: row.get(0)?,
                tag_id: row.get(1)?,
                alias: row.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    /// Returns the tag and every descendant, used by parent-tag filters.
    pub fn descendants(&self, id: i64) -> Result<Vec<i64>> {
        let mut statement = self.connection.prepare(
            "WITH RECURSIVE tree(id) AS (
                 SELECT ?1
                 UNION
                 SELECT tags.id FROM tags JOIN tree ON tags.parent_id = tree.id
             )
             SELECT id FROM tree ORDER BY id",
        )?;
        let rows = statement.query_map([id], |row| row.get::<_, i64>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    /// Merges the source tag into the target and removes the source.
    pub fn merge(&self, source_id: i64, target_id: i64) -> Result<Tag> {
        if source_id == target_id {
            return Err(StoreError::InvalidInput {
                field: "target_id",
                reason: "merge target must differ from the source".to_owned(),
            });
        }
        self.get(source_id)?;
        self.get(target_id)?;
        if self.descendants(source_id)?.contains(&target_id) {
            return Err(StoreError::InvalidInput {
                field: "target_id",
                reason: "cannot merge a tag into its own descendant".to_owned(),
            });
        }

        let transaction = self.connection.unchecked_transaction()?;
        let source = self.get(source_id)?;
        transaction.execute(
            "UPDATE OR IGNORE tag_aliases SET tag_id = ?1 WHERE tag_id = ?2",
            params![target_id, source_id],
        )?;
        transaction.execute("DELETE FROM tag_aliases WHERE tag_id = ?1", [source_id])?;
        transaction.execute(
            "INSERT OR IGNORE INTO tag_aliases (tag_id, alias) VALUES (?1, ?2)",
            params![target_id, source.canonical_name],
        )?;

        let mut statement = transaction.prepare(
            "SELECT asset_id, state, source, confidence, model_id, model_version
             FROM tag_assignments WHERE tag_id = ?1",
        )?;
        let assignments = statement
            .query_map([source_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<f64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);

        for (asset_id, state, assignment_source, confidence, model_id, model_version) in assignments
        {
            let existing = transaction
                .query_row(
                    "SELECT state FROM tag_assignments WHERE asset_id = ?1 AND tag_id = ?2",
                    params![asset_id, target_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            match existing {
                None => {
                    transaction.execute(
                        "INSERT INTO tag_assignments
                            (asset_id, tag_id, state, source, confidence, model_id, model_version)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            asset_id,
                            target_id,
                            state,
                            assignment_source,
                            confidence,
                            model_id,
                            model_version
                        ],
                    )?;
                }
                Some(existing) if rank(&existing) < rank(&state) => {
                    transaction.execute(
                        "UPDATE tag_assignments
                         SET state = ?1, source = ?2, confidence = ?3, model_id = ?4,
                             model_version = ?5, updated_at = unixepoch()
                         WHERE asset_id = ?6 AND tag_id = ?7",
                        params![
                            state,
                            assignment_source,
                            confidence,
                            model_id,
                            model_version,
                            asset_id,
                            target_id
                        ],
                    )?;
                }
                Some(_) => {}
            }
        }

        transaction.execute(
            "UPDATE tags SET parent_id = ?1 WHERE parent_id = ?2",
            params![target_id, source_id],
        )?;
        if let Some(parent_id) = source.parent_id {
            let target_parent: Option<i64> = transaction.query_row(
                "SELECT parent_id FROM tags WHERE id = ?1",
                [target_id],
                |row| row.get(0),
            )?;
            if target_parent.is_none() && parent_id != target_id {
                transaction.execute(
                    "UPDATE tags SET parent_id = ?1 WHERE id = ?2",
                    params![parent_id, target_id],
                )?;
            }
        }
        transaction.execute("DELETE FROM tags WHERE id = ?1", [source_id])?;
        transaction.commit()?;
        self.get(target_id)
    }
}

fn rank(state: &str) -> u8 {
    match state {
        "confirmed" => 3,
        "rejected" => 2,
        _ => 1,
    }
}

fn valid_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Debug, Clone, Copy)]
pub struct TagAssignmentRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> TagAssignmentRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn list_for_tag(&self, tag_id: i64, state: TagState) -> Result<Vec<TagAssignment>> {
        let mut statement = self.connection.prepare(
            "SELECT asset_id, tag_id, state, source, confidence, model_id, model_version, updated_at
             FROM tag_assignments WHERE tag_id = ?1 AND state = ?2 ORDER BY asset_id",
        )?;
        let rows = statement.query_map(params![tag_id, state.as_str()], assignment_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    pub fn confirmed_asset_ids_excluding(&self, tag_id: i64, limit: u32) -> Result<Vec<i64>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT asset_id FROM tag_assignments
             WHERE state = 'confirmed' AND tag_id != ?1
             ORDER BY asset_id LIMIT ?2",
        )?;
        let rows = statement.query_map(params![tag_id, limit], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    pub fn clear_suggested(
        &self,
        asset_id: i64,
        tag_id: i64,
        source: AssignmentSource,
    ) -> Result<()> {
        self.connection.execute(
            "DELETE FROM tag_assignments
             WHERE asset_id = ?1 AND tag_id = ?2 AND state = 'suggested' AND source = ?3",
            params![asset_id, tag_id, source.as_str()],
        )?;
        Ok(())
    }

    pub fn list_for_asset(&self, asset_id: i64) -> Result<Vec<TagAssignment>> {
        let mut statement = self.connection.prepare(
            "SELECT asset_id, tag_id, state, source, confidence, model_id, model_version, updated_at
             FROM tag_assignments WHERE asset_id = ?1 ORDER BY tag_id",
        )?;
        let rows = statement.query_map([asset_id], assignment_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    /// Applies a user decision. This is the only path allowed to write
    /// `confirmed` or `rejected`, and it always records `source = 'user'`.
    pub fn set_user(&self, asset_id: i64, tag_id: i64, state: TagState) -> Result<TagAssignment> {
        if state == TagState::Suggested {
            return Err(StoreError::InvalidInput {
                field: "state",
                reason: "user decisions must be confirmed or rejected".to_owned(),
            });
        }
        self.connection.execute(
            "INSERT INTO tag_assignments (asset_id, tag_id, state, source)
             VALUES (?1, ?2, ?3, 'user')
             ON CONFLICT (asset_id, tag_id) DO UPDATE SET
                 state = excluded.state,
                 source = 'user',
                 confidence = NULL,
                 model_id = NULL,
                 model_version = NULL,
                 updated_at = unixepoch()",
            params![asset_id, tag_id, state.as_str()],
        )?;
        self.get(asset_id, tag_id)
    }

    /// Applies one user decision to several assets atomically.
    pub fn set_user_many(&self, asset_ids: &[i64], tag_id: i64, state: TagState) -> Result<usize> {
        if state == TagState::Suggested {
            return Err(StoreError::InvalidInput {
                field: "state",
                reason: "user decisions must be confirmed or rejected".to_owned(),
            });
        }
        let transaction = self.connection.unchecked_transaction()?;
        let mut updated = 0;
        for asset_id in asset_ids {
            updated += transaction.execute(
                "INSERT INTO tag_assignments (asset_id, tag_id, state, source)
                 VALUES (?1, ?2, ?3, 'user')
                 ON CONFLICT (asset_id, tag_id) DO UPDATE SET
                     state = excluded.state,
                     source = 'user',
                     confidence = NULL,
                     model_id = NULL,
                     model_version = NULL,
                     updated_at = unixepoch()",
                params![asset_id, tag_id, state.as_str()],
            )?;
        }
        transaction.commit()?;
        Ok(updated)
    }

    /// Restores the assignment snapshot stored on a feedback event.
    pub fn restore(&self, asset_id: i64, tag_id: i64, event: &crate::FeedbackEvent) -> Result<()> {
        let Some(state) = event.previous_state else {
            self.connection.execute(
                "DELETE FROM tag_assignments WHERE asset_id = ?1 AND tag_id = ?2",
                params![asset_id, tag_id],
            )?;
            return Ok(());
        };
        let source = event.previous_source.unwrap_or(AssignmentSource::User);
        self.connection.execute(
            "INSERT INTO tag_assignments
                (asset_id, tag_id, state, source, confidence, model_id, model_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (asset_id, tag_id) DO UPDATE SET
                 state = excluded.state,
                 source = excluded.source,
                 confidence = excluded.confidence,
                 model_id = excluded.model_id,
                 model_version = excluded.model_version,
                 updated_at = unixepoch()",
            params![
                asset_id,
                tag_id,
                state.as_str(),
                source.as_str(),
                event.previous_confidence,
                event.previous_model_id,
                event.previous_model_version
            ],
        )?;
        Ok(())
    }

    /// Removes a user assignment from several assets atomically.
    pub fn remove_user_many(&self, asset_ids: &[i64], tag_id: i64) -> Result<usize> {
        let transaction = self.connection.unchecked_transaction()?;
        let mut removed = 0;
        for asset_id in asset_ids {
            removed += transaction.execute(
                "DELETE FROM tag_assignments
                 WHERE asset_id = ?1 AND tag_id = ?2 AND source = 'user'",
                params![asset_id, tag_id],
            )?;
        }
        transaction.commit()?;
        Ok(removed)
    }

    /// Deletes only user-owned assignments, leaving machine suggestions intact.
    pub fn remove_user(&self, asset_id: i64, tag_id: i64) -> Result<bool> {
        Ok(self.connection.execute(
            "DELETE FROM tag_assignments
             WHERE asset_id = ?1 AND tag_id = ?2 AND source = 'user'",
            params![asset_id, tag_id],
        )? == 1)
    }

    /// Writes a machine suggestion. Confirmed, rejected, and other sources'
    /// suggestions are never overwritten.
    pub fn upsert_suggested(
        &self,
        asset_id: i64,
        tag_id: i64,
        source: AssignmentSource,
        confidence: f64,
        model_id: &str,
        model_version: &str,
    ) -> Result<Option<TagAssignment>> {
        if source == AssignmentSource::User {
            return Err(StoreError::InvalidInput {
                field: "source",
                reason: "machine suggestions cannot use the user source".to_owned(),
            });
        }
        if !(0.0..=1.0).contains(&confidence) || !confidence.is_finite() {
            return Err(StoreError::InvalidInput {
                field: "confidence",
                reason: "must be between 0 and 1".to_owned(),
            });
        }
        validate_nonempty("model_id", model_id)?;
        validate_nonempty("model_version", model_version)?;
        self.connection.execute(
            "INSERT INTO tag_assignments
                (asset_id, tag_id, state, source, confidence, model_id, model_version)
             VALUES (?1, ?2, 'suggested', ?3, ?4, ?5, ?6)
             ON CONFLICT (asset_id, tag_id) DO UPDATE SET
                 confidence = excluded.confidence,
                 model_id = excluded.model_id,
                 model_version = excluded.model_version,
                 updated_at = unixepoch()
             WHERE tag_assignments.state = 'suggested'
               AND tag_assignments.source = excluded.source",
            params![
                asset_id,
                tag_id,
                source.as_str(),
                confidence,
                model_id,
                model_version
            ],
        )?;
        self.get_optional(asset_id, tag_id)
    }

    /// Replaces one source's suggestions for an asset during recalculation.
    pub fn replace_suggestions(
        &self,
        asset_id: i64,
        source: AssignmentSource,
        model_id: &str,
        model_version: &str,
        suggestions: &[(i64, f64)],
    ) -> Result<()> {
        if source == AssignmentSource::User {
            return Err(StoreError::InvalidInput {
                field: "source",
                reason: "machine suggestions cannot use the user source".to_owned(),
            });
        }
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM tag_assignments
             WHERE asset_id = ?1 AND state = 'suggested' AND source = ?2",
            params![asset_id, source.as_str()],
        )?;
        for (tag_id, confidence) in suggestions {
            if !(0.0..=1.0).contains(confidence) || !confidence.is_finite() {
                return Err(StoreError::InvalidInput {
                    field: "confidence",
                    reason: "must be between 0 and 1".to_owned(),
                });
            }
            transaction.execute(
                "INSERT INTO tag_assignments
                    (asset_id, tag_id, state, source, confidence, model_id, model_version)
                 VALUES (?1, ?2, 'suggested', ?3, ?4, ?5, ?6)
                 ON CONFLICT (asset_id, tag_id) DO NOTHING",
                params![
                    asset_id,
                    tag_id,
                    source.as_str(),
                    confidence,
                    model_id,
                    model_version
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn get(&self, asset_id: i64, tag_id: i64) -> Result<TagAssignment> {
        Ok(self.connection.query_row(
            "SELECT asset_id, tag_id, state, source, confidence, model_id, model_version, updated_at
             FROM tag_assignments WHERE asset_id = ?1 AND tag_id = ?2",
            params![asset_id, tag_id],
            assignment_from_row,
        )?)
    }

    pub fn get_optional(&self, asset_id: i64, tag_id: i64) -> Result<Option<TagAssignment>> {
        Ok(self
            .connection
            .query_row(
                "SELECT asset_id, tag_id, state, source, confidence, model_id, model_version, updated_at
                 FROM tag_assignments WHERE asset_id = ?1 AND tag_id = ?2",
                params![asset_id, tag_id],
                assignment_from_row,
            )
            .optional()?)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AssetFilterRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> AssetFilterRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn page(&self, filter: &AssetFilter, after_id: i64, limit: u32) -> Result<Vec<Asset>> {
        let (clause, values) = self.build_clause(filter, Some(after_id))?;
        let sql = format!(
            "SELECT id FROM assets {clause} ORDER BY id LIMIT ?{}",
            values.len() + 1
        );
        let mut statement = self.connection.prepare(&sql)?;
        let mut bound = values;
        bound.push(Value::Integer(i64::from(limit)));
        let ids = statement
            .query_map(params_from_iter(bound), |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut assets = Vec::with_capacity(ids.len());
        for id in ids {
            assets.push(self.asset(id)?);
        }
        Ok(assets)
    }

    pub fn count(&self, filter: &AssetFilter) -> Result<u32> {
        let (clause, values) = self.build_clause(filter, None)?;
        let sql = format!("SELECT COUNT(*) FROM assets {clause}");
        Ok(self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get(0))?)
    }

    fn asset(&self, id: i64) -> Result<Asset> {
        Ok(self.connection.query_row(
            "SELECT id, blake3_hash, file_size, width, height, format, captured_at, perceptual_hash
             FROM assets WHERE id = ?1",
            [id],
            |row| {
                let raw_hash: Vec<u8> = row.get(1)?;
                let hash: [u8; 32] = raw_hash.try_into().map_err(|value: Vec<u8>| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Blob,
                        Box::new(StoreError::CorruptData {
                            field: "blake3_hash",
                            value: format!("{} bytes", value.len()),
                        }),
                    )
                })?;
                let raw_phash: Option<Vec<u8>> = row.get(7)?;
                let perceptual_hash = raw_phash
                    .map(|value| {
                        value.try_into().map_err(|value: Vec<u8>| {
                            rusqlite::Error::FromSqlConversionFailure(
                                7,
                                rusqlite::types::Type::Blob,
                                Box::new(StoreError::CorruptData {
                                    field: "perceptual_hash",
                                    value: format!("{} bytes", value.len()),
                                }),
                            )
                        })
                    })
                    .transpose()?;
                Ok(Asset {
                    id: row.get(0)?,
                    blake3_hash: hash,
                    file_size: row.get(2)?,
                    width: row.get(3)?,
                    height: row.get(4)?,
                    format: row.get(5)?,
                    captured_at: row.get(6)?,
                    perceptual_hash,
                })
            },
        )?)
    }

    fn build_clause(
        &self,
        filter: &AssetFilter,
        after_id: Option<i64>,
    ) -> Result<(String, Vec<Value>)> {
        let mut conditions = Vec::new();
        let mut values = Vec::new();
        if let Some(after_id) = after_id {
            conditions.push("id > ?".to_owned());
            values.push(Value::Integer(after_id));
        }

        if !filter.formats.is_empty() {
            let placeholders = vec!["?"; filter.formats.len()].join(", ");
            conditions.push(format!("lower(format) IN ({placeholders})"));
            for format in &filter.formats {
                values.push(Value::Text(format.to_lowercase()));
            }
        }
        if let Some(width) = filter.min_width {
            conditions.push("width >= ?".to_owned());
            values.push(Value::Integer(i64::from(width)));
        }
        if let Some(height) = filter.min_height {
            conditions.push("height >= ?".to_owned());
            values.push(Value::Integer(i64::from(height)));
        }
        if let Some(from) = filter.captured_from {
            conditions.push("captured_at >= ?".to_owned());
            values.push(Value::Integer(from));
        }
        if let Some(to) = filter.captured_to {
            conditions.push("captured_at <= ?".to_owned());
            values.push(Value::Integer(to));
        }

        if let Some(asset_ids) = &filter.asset_ids {
            let ids = asset_ids
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            conditions.push("id IN (SELECT value FROM json_each(?))".to_owned());
            values.push(Value::Text(format!("[{ids}]")));
        }

        let states = if filter.tags.include_suggested {
            "'confirmed', 'suggested'"
        } else {
            "'confirmed'"
        };
        let has_any_of = |ids: &[i64], values: &mut Vec<Value>| -> String {
            let placeholders = vec!["?"; ids.len()].join(", ");
            values.extend(ids.iter().map(|id| Value::Integer(*id)));
            format!(
                "EXISTS (SELECT 1 FROM tag_assignments
                 WHERE tag_assignments.asset_id = assets.id
                   AND tag_assignments.state IN ({states})
                   AND tag_assignments.tag_id IN ({placeholders}))"
            )
        };
        for tag_id in &filter.tags.all {
            let ids = self.descendants(*tag_id)?;
            conditions.push(has_any_of(&ids, &mut values));
        }
        if !filter.tags.any.is_empty() {
            let ids = self.descendants_of_all(&filter.tags.any)?;
            conditions.push(has_any_of(&ids, &mut values));
        }
        if !filter.tags.none.is_empty() {
            let ids = self.descendants_of_all(&filter.tags.none)?;
            let exists = has_any_of(&ids, &mut values);
            conditions.push(format!("NOT {exists}"));
        }

        let clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        Ok((clause, values))
    }

    fn descendants_of_all(&self, tag_ids: &[i64]) -> Result<Vec<i64>> {
        let mut ids = Vec::new();
        for tag_id in tag_ids {
            ids.extend(self.descendants(*tag_id)?);
        }
        Ok(unique(ids))
    }

    fn descendants(&self, tag_id: i64) -> Result<Vec<i64>> {
        let mut statement = self.connection.prepare(
            "WITH RECURSIVE tree(id) AS (
                 SELECT ?1
                 UNION
                 SELECT tags.id FROM tags JOIN tree ON tags.parent_id = tree.id
             )
             SELECT id FROM tree",
        )?;
        let rows = statement.query_map([tag_id], |row| row.get::<_, i64>(0))?;
        let ids = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        if ids.len() > 900 {
            return Err(StoreError::InvalidInput {
                field: "tag hierarchy",
                reason: "too many descendants for one filter".to_owned(),
            });
        }
        Ok(unique(ids))
    }
}

fn unique(mut ids: Vec<i64>) -> Vec<i64> {
    ids.sort_unstable();
    ids.dedup();
    ids
}
