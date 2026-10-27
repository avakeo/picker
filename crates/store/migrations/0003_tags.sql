CREATE TABLE tags (
    id INTEGER PRIMARY KEY,
    namespace TEXT NOT NULL DEFAULT 'user' CHECK (length(namespace) > 0),
    canonical_name TEXT NOT NULL CHECK (length(trim(canonical_name)) > 0),
    display_name_ja TEXT NOT NULL CHECK (length(trim(display_name_ja)) > 0),
    display_name_en TEXT,
    parent_id INTEGER REFERENCES tags(id) ON DELETE SET NULL,
    color TEXT CHECK (
        color IS NULL OR (
            length(color) = 7
            AND substr(color, 1, 1) = '#'
            AND substr(color, 2, 1) GLOB '[0-9A-Fa-f]'
            AND substr(color, 3, 1) GLOB '[0-9A-Fa-f]'
            AND substr(color, 4, 1) GLOB '[0-9A-Fa-f]'
            AND substr(color, 5, 1) GLOB '[0-9A-Fa-f]'
            AND substr(color, 6, 1) GLOB '[0-9A-Fa-f]'
            AND substr(color, 7, 1) GLOB '[0-9A-Fa-f]'
        )
    ),
    origin TEXT NOT NULL DEFAULT 'user'
        CHECK (origin IN ('user', 'zero_shot', 'custom_classifier', 'vision', 'danbooru', 'rating', 'laya')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (namespace, canonical_name),
    CHECK (parent_id IS NULL OR parent_id != id)
) STRICT;

CREATE INDEX tags_parent_id_idx ON tags(parent_id) WHERE parent_id IS NOT NULL;

CREATE TABLE tag_aliases (
    id INTEGER PRIMARY KEY,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    alias TEXT NOT NULL CHECK (length(trim(alias)) > 0),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (tag_id, alias),
    UNIQUE (alias)
) STRICT;

CREATE INDEX tag_aliases_tag_id_idx ON tag_aliases(tag_id);

CREATE TABLE tag_assignments (
    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN ('suggested', 'confirmed', 'rejected')),
    source TEXT NOT NULL
        CHECK (source IN ('zero_shot', 'custom_classifier', 'user', 'vision', 'danbooru', 'rating', 'laya')),
    confidence REAL CHECK (confidence IS NULL OR (confidence >= 0.0 AND confidence <= 1.0)),
    model_id TEXT,
    model_version TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (asset_id, tag_id),
    CHECK (state = 'suggested' OR source = 'user'),
    CHECK ((model_id IS NULL) = (model_version IS NULL)),
    CHECK (state != 'suggested' OR confidence IS NOT NULL)
) STRICT;

CREATE INDEX tag_assignments_tag_state_idx ON tag_assignments(tag_id, state);
CREATE INDEX tag_assignments_asset_state_idx ON tag_assignments(asset_id, state);
