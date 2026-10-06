BEGIN;
-- Trusted catalog observations: evidence only, never display overrides.
CREATE TABLE album_provider_evidence (
 album_id TEXT NOT NULL REFERENCES album(id) ON DELETE CASCADE,
 provider TEXT NOT NULL, kind TEXT NOT NULL, external_id TEXT NOT NULL,
 title TEXT NOT NULL, release_date TEXT, release_type TEXT,
 PRIMARY KEY(album_id,provider,kind,external_id)
) STRICT;
CREATE TABLE track_provider_evidence (
 track_id TEXT NOT NULL REFERENCES track(id) ON DELETE CASCADE,
 provider TEXT NOT NULL, kind TEXT NOT NULL, external_id TEXT NOT NULL,
 title TEXT NOT NULL, disc INTEGER, position INTEGER, duration_ms INTEGER,
 PRIMARY KEY(track_id,provider,kind,external_id)
) STRICT;
PRAGMA user_version=28;
COMMIT;
