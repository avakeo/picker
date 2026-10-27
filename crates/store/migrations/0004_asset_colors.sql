CREATE TABLE asset_colors (
    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    rank INTEGER NOT NULL CHECK (rank BETWEEN 1 AND 3),
    hex TEXT NOT NULL CHECK (
        length(hex) = 7
        AND substr(hex, 1, 1) = '#'
        AND substr(hex, 2, 1) GLOB '[0-9A-Fa-f]'
        AND substr(hex, 3, 1) GLOB '[0-9A-Fa-f]'
        AND substr(hex, 4, 1) GLOB '[0-9A-Fa-f]'
        AND substr(hex, 5, 1) GLOB '[0-9A-Fa-f]'
        AND substr(hex, 6, 1) GLOB '[0-9A-Fa-f]'
        AND substr(hex, 7, 1) GLOB '[0-9A-Fa-f]'
    ),
    ratio REAL NOT NULL CHECK (ratio >= 0.0 AND ratio <= 1.0),
    source TEXT NOT NULL CHECK (source IN ('auto', 'user')),
    algorithm_version TEXT NOT NULL CHECK (length(algorithm_version) > 0),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (asset_id, rank)
) STRICT;

CREATE TABLE asset_color_manual (
    asset_id INTEGER PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
