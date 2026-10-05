BEGIN;
ALTER TABLE playlist_source RENAME TO playlist_source_old;
CREATE TABLE playlist_source (
    playlist_id TEXT NOT NULL REFERENCES playlist(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    external_id TEXT NOT NULL,
    source_url TEXT NOT NULL,
    version TEXT,
    owner TEXT NOT NULL DEFAULT '',
    PRIMARY KEY(playlist_id,provider)
) STRICT;
INSERT INTO playlist_source SELECT * FROM playlist_source_old;
DROP TABLE playlist_source_old;
CREATE INDEX playlist_source_external ON playlist_source(provider,external_id);
PRAGMA user_version=24;
COMMIT;
