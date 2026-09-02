PRAGMA foreign_keys = ON;
BEGIN;

CREATE TABLE discovery_root (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind = 'local_filesystem'),
    location BLOB NOT NULL UNIQUE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE scan_run (
    id INTEGER PRIMARY KEY,
    root_id TEXT NOT NULL REFERENCES discovery_root(id) ON DELETE CASCADE,
    started_at INTEGER NOT NULL DEFAULT (unixepoch()),
    completed_at INTEGER,
    status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'failed'))
) STRICT;
CREATE INDEX scan_run_root_status ON scan_run(root_id, status);

CREATE TABLE release (
    id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE release_application_metadata (
    release_id TEXT PRIMARY KEY REFERENCES release(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    year INTEGER
) STRICT;

CREATE TABLE track (
    id TEXT PRIMARY KEY,
    release_id TEXT NOT NULL REFERENCES release(id) ON DELETE CASCADE,
    disc_number INTEGER,
    track_number INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE INDEX track_release_order ON track(release_id, disc_number, track_number, id);

CREATE TABLE track_application_metadata (
    track_id TEXT PRIMARY KEY REFERENCES track(id) ON DELETE CASCADE,
    title TEXT NOT NULL
) STRICT;

CREATE TABLE artist (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE track_artist_credit (
    track_id TEXT NOT NULL REFERENCES track(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    artist_id TEXT NOT NULL REFERENCES artist(id),
    role TEXT,
    PRIMARY KEY (track_id, position)
) STRICT;
CREATE INDEX track_artist_credit_artist ON track_artist_credit(artist_id, track_id);

CREATE TABLE release_artist_credit (
    release_id TEXT NOT NULL REFERENCES release(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    artist_id TEXT NOT NULL REFERENCES artist(id),
    role TEXT,
    PRIMARY KEY (release_id, position)
) STRICT;
CREATE INDEX release_artist_credit_artist ON release_artist_credit(artist_id, release_id);

CREATE TABLE library_membership (
    track_id TEXT PRIMARY KEY REFERENCES track(id) ON DELETE CASCADE,
    added_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE playable_source (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE track_source (
    track_id TEXT NOT NULL REFERENCES track(id) ON DELETE CASCADE,
    source_id TEXT NOT NULL UNIQUE REFERENCES playable_source(id) ON DELETE CASCADE,
    associated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (track_id, source_id)
) STRICT;

CREATE TABLE local_file_observation (
    source_id TEXT PRIMARY KEY REFERENCES playable_source(id) ON DELETE CASCADE,
    root_id TEXT NOT NULL REFERENCES discovery_root(id) ON DELETE CASCADE,
    path BLOB NOT NULL,
    size_bytes INTEGER NOT NULL,
    modified_ns INTEGER NOT NULL,
    available INTEGER NOT NULL CHECK (available IN (0, 1)),
    last_seen_scan_id INTEGER REFERENCES scan_run(id),
    last_observed_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (root_id, path)
) STRICT;
CREATE INDEX local_file_root_seen ON local_file_observation(root_id, last_seen_scan_id);
CREATE INDEX local_file_available ON local_file_observation(available, source_id);

CREATE TABLE file_metadata_observation (
    source_id TEXT PRIMARY KEY REFERENCES playable_source(id) ON DELETE CASCADE,
    track_title TEXT,
    release_title TEXT,
    disc_number INTEGER,
    track_number INTEGER,
    year INTEGER,
    duration_ms INTEGER,
    format TEXT,
    observed_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE file_artist_observation (
    source_id TEXT NOT NULL REFERENCES playable_source(id) ON DELETE CASCADE,
    scope TEXT NOT NULL CHECK (scope IN ('track', 'release')),
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    PRIMARY KEY (source_id, scope, position)
) STRICT;

CREATE TABLE track_title_override (
    track_id TEXT PRIMARY KEY REFERENCES track(id) ON DELETE CASCADE,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE effective_track_metadata (
    track_id TEXT PRIMARY KEY REFERENCES track(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    release_title TEXT NOT NULL,
    artist_names TEXT NOT NULL,
    year INTEGER,
    duration_ms INTEGER,
    format TEXT
) STRICT;
CREATE INDEX effective_track_title ON effective_track_metadata(title, track_id);
CREATE INDEX effective_track_format ON effective_track_metadata(format, track_id);
CREATE INDEX effective_track_year ON effective_track_metadata(year, track_id);

CREATE VIRTUAL TABLE track_search USING fts5(
    track_id UNINDEXED,
    title,
    artist_names,
    release_title,
    tokenize = 'unicode61 remove_diacritics 2'
);

COMMIT;
