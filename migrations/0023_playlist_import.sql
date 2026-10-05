BEGIN;
CREATE TABLE playlist_source (
    playlist_id TEXT NOT NULL REFERENCES playlist(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    external_id TEXT NOT NULL,
    source_url TEXT NOT NULL,
    version TEXT,
    owner TEXT NOT NULL DEFAULT '',
    PRIMARY KEY(provider,external_id),
    UNIQUE(playlist_id,provider)
) STRICT;
PRAGMA user_version=23;
COMMIT;
