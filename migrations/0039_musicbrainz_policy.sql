BEGIN;
-- Automatic completion receipts are distinct from manual inspection and raw observations.
CREATE TABLE IF NOT EXISTS musicbrainz_policy (
 album_id TEXT PRIMARY KEY REFERENCES album(id) ON DELETE CASCADE,
 evaluation_version INTEGER NOT NULL,
 discovery_version INTEGER NOT NULL,
 evaluated_at INTEGER NOT NULL DEFAULT(unixepoch())
) STRICT;
PRAGMA user_version=39;
COMMIT;
