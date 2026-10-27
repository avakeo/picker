CREATE TABLE roots (
    id INTEGER PRIMARY KEY,
    absolute_path TEXT NOT NULL UNIQUE CHECK (length(absolute_path) > 0),
    volume_id TEXT,
    display_name TEXT NOT NULL CHECK (length(display_name) > 0),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    status TEXT NOT NULL DEFAULT 'online' CHECK (status IN ('online', 'offline')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE assets (
    id INTEGER PRIMARY KEY,
    blake3_hash BLOB NOT NULL UNIQUE CHECK (length(blake3_hash) = 32),
    file_size INTEGER NOT NULL CHECK (file_size >= 0),
    width INTEGER CHECK (width > 0),
    height INTEGER CHECK (height > 0),
    format TEXT,
    captured_at INTEGER,
    perceptual_hash BLOB CHECK (perceptual_hash IS NULL OR length(perceptual_hash) = 8),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE locations (
    id INTEGER PRIMARY KEY,
    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    root_id INTEGER NOT NULL REFERENCES roots(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL CHECK (length(relative_path) > 0),
    modified_at INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'present' CHECK (status IN ('present', 'missing', 'offline')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (root_id, relative_path)
) STRICT;

CREATE INDEX locations_asset_id_idx ON locations(asset_id);
CREATE INDEX locations_root_status_idx ON locations(root_id, status);

CREATE TABLE stage_states (
    id INTEGER PRIMARY KEY,
    asset_id INTEGER REFERENCES assets(id) ON DELETE CASCADE,
    location_id INTEGER REFERENCES locations(id) ON DELETE CASCADE,
    stage TEXT NOT NULL CHECK (stage IN ('hash', 'metadata', 'thumbnail', 'phash', 'embedding', 'auto_tag', 'custom_suggest')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'succeeded', 'failed')),
    version TEXT NOT NULL CHECK (length(version) > 0),
    error TEXT,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK ((asset_id IS NOT NULL) != (location_id IS NOT NULL)),
    CHECK ((status = 'failed') OR error IS NULL)
) STRICT;

CREATE UNIQUE INDEX stage_states_asset_stage_idx
    ON stage_states(asset_id, stage) WHERE asset_id IS NOT NULL;
CREATE UNIQUE INDEX stage_states_location_stage_idx
    ON stage_states(location_id, stage) WHERE location_id IS NOT NULL;

CREATE TABLE jobs (
    id INTEGER PRIMARY KEY,
    asset_id INTEGER REFERENCES assets(id) ON DELETE CASCADE,
    location_id INTEGER REFERENCES locations(id) ON DELETE CASCADE,
    stage TEXT NOT NULL CHECK (stage IN ('hash', 'metadata', 'thumbnail', 'phash', 'embedding', 'auto_tag', 'custom_suggest')),
    priority INTEGER NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    max_attempts INTEGER NOT NULL DEFAULT 3 CHECK (max_attempts > 0),
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'paused', 'succeeded', 'failed')),
    last_error TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK ((asset_id IS NOT NULL) != (location_id IS NOT NULL)),
    CHECK (attempts <= max_attempts)
) STRICT;

CREATE INDEX jobs_dequeue_idx ON jobs(status, stage, priority DESC, created_at, id);
CREATE INDEX jobs_asset_id_idx ON jobs(asset_id) WHERE asset_id IS NOT NULL;
CREATE INDEX jobs_location_id_idx ON jobs(location_id) WHERE location_id IS NOT NULL;

CREATE TABLE settings (
    key TEXT PRIMARY KEY CHECK (length(key) > 0),
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
