CREATE TABLE pending_tag_assignments (
    blake3_hash BLOB NOT NULL CHECK (length(blake3_hash) = 32),
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN ('suggested', 'confirmed', 'rejected')),
    source TEXT NOT NULL CHECK (source IN ('zero_shot', 'custom_classifier', 'user', 'vision', 'danbooru', 'rating', 'laya')),
    confidence REAL CHECK (confidence IS NULL OR (confidence >= 0.0 AND confidence <= 1.0)),
    model_id TEXT,
    model_version TEXT,
    PRIMARY KEY (blake3_hash, tag_id)
) STRICT;

CREATE INDEX pending_tag_assignments_hash_idx ON pending_tag_assignments(blake3_hash);
