use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{AssignmentSource, FeedbackAction, FeedbackEvent, Result, StoreError, TagState};

#[derive(Debug, Clone, Copy)]
pub struct FeedbackRepository<'connection> {
    connection: &'connection Connection,
}

fn state_to_text(state: TagState) -> &'static str {
    state.as_str()
}

fn parse_state(value: Option<String>) -> rusqlite::Result<Option<TagState>> {
    value
        .map(|value| value.parse().map_err(|_| rusqlite::Error::InvalidQuery))
        .transpose()
}

fn parse_source(value: Option<String>) -> rusqlite::Result<Option<AssignmentSource>> {
    value
        .map(|value| value.parse().map_err(|_| rusqlite::Error::InvalidQuery))
        .transpose()
}

fn event_from_row(row: &Row<'_>) -> rusqlite::Result<FeedbackEvent> {
    let action: String = row.get(3)?;
    let previous_state: Option<String> = row.get(5)?;
    let previous_source: Option<String> = row.get(6)?;
    Ok(FeedbackEvent {
        id: row.get(0)?,
        asset_id: row.get(1)?,
        tag_id: row.get(2)?,
        action: action.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
        confidence: row.get(4)?,
        previous_state: parse_state(previous_state)?,
        previous_source: parse_source(previous_source)?,
        previous_confidence: row.get(7)?,
        previous_model_id: row.get(8)?,
        previous_model_version: row.get(9)?,
        created_at: row.get(10)?,
    })
}

impl<'connection> FeedbackRepository<'connection> {
    pub(crate) const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        asset_id: i64,
        tag_id: i64,
        action: FeedbackAction,
        confidence: Option<f64>,
        previous: Option<&crate::TagAssignment>,
    ) -> Result<FeedbackEvent> {
        self.connection.execute(
            "INSERT INTO feedback_events
                (asset_id, tag_id, action, confidence, previous_state, previous_source,
                 previous_confidence, previous_model_id, previous_model_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                asset_id,
                tag_id,
                action.as_str(),
                confidence,
                previous.map(|value| state_to_text(value.state)),
                previous.map(|value| value.source.as_str()),
                previous.and_then(|value| value.confidence),
                previous.and_then(|value| value.model_id.as_deref()),
                previous.and_then(|value| value.model_version.as_deref()),
            ],
        )?;
        self.connection
            .query_row(
                "SELECT id, asset_id, tag_id, action, confidence, previous_state,
                        previous_source, previous_confidence, previous_model_id,
                        previous_model_version, created_at
                 FROM feedback_events WHERE id = ?1",
                [self.connection.last_insert_rowid()],
                event_from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::CorruptData {
                field: "feedback_event",
                value: "inserted row was not found".to_owned(),
            })
    }

    /// Inserts an event restored from an export, keeping its original time.
    /// An identical event already present is not inserted again.
    pub fn restore(&self, event: &FeedbackEvent) -> Result<bool> {
        let inserted = self.connection.execute(
            "INSERT INTO feedback_events
                (asset_id, tag_id, action, confidence, previous_state, previous_source,
                 previous_confidence, previous_model_id, previous_model_version, created_at)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10
             WHERE NOT EXISTS (
                 SELECT 1 FROM feedback_events
                 WHERE asset_id = ?1 AND tag_id IS ?2 AND action = ?3 AND created_at = ?10
             )",
            params![
                event.asset_id,
                event.tag_id,
                event.action.as_str(),
                event.confidence,
                event.previous_state.map(state_to_text),
                event.previous_source.map(AssignmentSource::as_str),
                event.previous_confidence,
                event.previous_model_id,
                event.previous_model_version,
                event.created_at,
            ],
        )?;
        Ok(inserted > 0)
    }

    /// Returns every event in recording order.
    pub fn list_all(&self) -> Result<Vec<FeedbackEvent>> {
        self.list_since(0, u32::MAX)
    }

    pub fn get(&self, id: i64) -> Result<Option<FeedbackEvent>> {
        self.connection
            .query_row(
                "SELECT id, asset_id, tag_id, action, confidence, previous_state,
                        previous_source, previous_confidence, previous_model_id,
                        previous_model_version, created_at
                 FROM feedback_events WHERE id = ?1",
                [id],
                event_from_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn list_for_asset(&self, asset_id: i64) -> Result<Vec<FeedbackEvent>> {
        let mut statement = self.connection.prepare(
            "SELECT id, asset_id, tag_id, action, confidence, previous_state,
                    previous_source, previous_confidence, previous_model_id,
                    previous_model_version, created_at
             FROM feedback_events WHERE asset_id = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map([asset_id], event_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }

    pub fn list_since(&self, after_id: i64, limit: u32) -> Result<Vec<FeedbackEvent>> {
        let mut statement = self.connection.prepare(
            "SELECT id, asset_id, tag_id, action, confidence, previous_state,
                    previous_source, previous_confidence, previous_model_id,
                    previous_model_version, created_at
             FROM feedback_events WHERE id > ?1 ORDER BY id LIMIT ?2",
        )?;
        let rows = statement.query_map(params![after_id, limit], event_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StoreError::from)
    }
}
