CREATE TABLE pending_portable_items (
    id INTEGER PRIMARY KEY,
    blake3_hash BLOB NOT NULL CHECK (length(blake3_hash) = 32),
    kind TEXT NOT NULL CHECK (kind IN ('color', 'album_item', 'feedback')),
    payload TEXT NOT NULL CHECK (json_valid(payload))
) STRICT;

CREATE INDEX pending_portable_items_hash_idx ON pending_portable_items(blake3_hash, id);
