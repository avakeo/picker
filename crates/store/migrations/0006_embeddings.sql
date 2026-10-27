CREATE TABLE embeddings (
    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    model_id TEXT NOT NULL CHECK (length(model_id) > 0),
    model_version TEXT NOT NULL CHECK (length(model_version) > 0),
    dimensions INTEGER NOT NULL CHECK (dimensions BETWEEN 1 AND 4096),
    vector BLOB NOT NULL CHECK (length(vector) = dimensions * 4),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (asset_id, model_id, model_version)
) STRICT;

CREATE INDEX embeddings_model_idx ON embeddings(model_id, model_version);
