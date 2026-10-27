CREATE TABLE feedback_events (
    id INTEGER PRIMARY KEY,
    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    tag_id INTEGER REFERENCES tags(id) ON DELETE SET NULL,
    action TEXT NOT NULL CHECK (action IN ('approved', 'rejected', 'manual_add', 'manual_remove', 'undo')),
    confidence REAL CHECK (confidence IS NULL OR (confidence >= 0.0 AND confidence <= 1.0)),
    previous_state TEXT CHECK (previous_state IS NULL OR previous_state IN ('suggested', 'confirmed', 'rejected')),
    previous_source TEXT CHECK (previous_source IS NULL OR previous_source IN ('zero_shot', 'custom_classifier', 'user', 'vision', 'danbooru', 'rating', 'laya')),
    previous_confidence REAL CHECK (previous_confidence IS NULL OR (previous_confidence >= 0.0 AND previous_confidence <= 1.0)),
    previous_model_id TEXT,
    previous_model_version TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE INDEX feedback_events_asset_idx ON feedback_events(asset_id, id);
CREATE INDEX feedback_events_tag_idx ON feedback_events(tag_id, id);
