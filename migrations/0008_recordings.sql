-- Same parent-rebuild convention as 0004: preserve every child and Track ID.
-- Store owns the IMMEDIATE transaction and populates recording_backfill with
-- deterministic UUIDv5 IDs from application Track IDs (never provider metadata).
CREATE TABLE recording (
    id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE TABLE recording_external_identity (
    recording_id TEXT NOT NULL REFERENCES recording(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY (recording_id, provider, kind, external_id)
) STRICT;
CREATE INDEX recording_external_identity_lookup ON recording_external_identity(provider, kind, external_id);
INSERT INTO recording(id, created_at)
SELECT b.recording_id, t.created_at FROM recording_backfill b JOIN track t ON t.id=b.track_id;
-- Preserve historical Track associations while copying already-established evidence.
INSERT INTO recording_external_identity
SELECT b.recording_id, i.provider, i.kind, i.external_id FROM recording_backfill b
JOIN track_external_identity i ON i.track_id=b.track_id
WHERE i.kind='recording' AND i.provider IN ('musicbrainz', 'isrc');
CREATE TABLE track_new (
    id TEXT PRIMARY KEY,
    release_id TEXT NOT NULL REFERENCES release(id) ON DELETE CASCADE,
    recording_id TEXT NOT NULL REFERENCES recording(id),
    disc_number INTEGER,
    track_number INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
INSERT INTO track_new(id, release_id, recording_id, disc_number, track_number, created_at)
SELECT t.id, t.release_id, b.recording_id, t.disc_number, t.track_number, t.created_at
FROM track t JOIN recording_backfill b ON b.track_id=t.id;
DROP TABLE track;
ALTER TABLE track_new RENAME TO track;
CREATE INDEX track_release_order ON track(release_id, disc_number, track_number, id);
CREATE INDEX track_recording ON track(recording_id, id);
DROP TABLE recording_backfill;
CREATE TEMP TABLE recording_migration_integrity(valid INTEGER CHECK(valid=1));
INSERT INTO recording_migration_integrity SELECT 0 FROM pragma_foreign_key_check;
DROP TABLE recording_migration_integrity;
PRAGMA user_version = 8;
