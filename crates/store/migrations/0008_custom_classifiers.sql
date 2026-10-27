CREATE TABLE custom_classifiers (
    tag_id INTEGER PRIMARY KEY REFERENCES tags(id) ON DELETE CASCADE,
    dimensions INTEGER NOT NULL CHECK (dimensions > 0),
    weights BLOB NOT NULL CHECK (length(weights) = dimensions * 8),
    bias REAL NOT NULL,
    trained_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
