-- Rebuild the parent without rewriting or cascading its existing child references.
PRAGMA foreign_keys = OFF;
BEGIN IMMEDIATE;
CREATE TABLE album (
    id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE TABLE album_application_metadata (
    album_id TEXT PRIMARY KEY REFERENCES album(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    year INTEGER
) STRICT;
CREATE TABLE album_artist_credit (
    album_id TEXT NOT NULL REFERENCES album(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    artist_id TEXT NOT NULL REFERENCES artist(id),
    role TEXT,
    join_phrase TEXT,
    PRIMARY KEY (album_id, position)
) STRICT;
CREATE INDEX album_artist_credit_artist ON album_artist_credit(artist_id, album_id);
CREATE TABLE album_external_identity (
    album_id TEXT NOT NULL REFERENCES album(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY (album_id, provider, kind, external_id)
) STRICT;
CREATE INDEX album_external_identity_lookup ON album_external_identity(provider, kind, external_id);

-- Independent random application IDs: no metadata matching and no Release ID reuse.
CREATE TEMP TABLE album_backfill (release_id TEXT PRIMARY KEY, album_id TEXT NOT NULL);
INSERT INTO album_backfill
SELECT id, lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' ||
    substr(hex(randomblob(2)), 2) || '-8' || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6)))
FROM release;
INSERT INTO album(id, created_at)
SELECT b.album_id, r.created_at FROM album_backfill b JOIN release r ON r.id = b.release_id;
INSERT INTO album_application_metadata(album_id, title, year)
SELECT b.album_id, COALESCE(m.title, (SELECT e.release_title FROM track t JOIN effective_track_metadata e ON e.track_id=t.id WHERE t.release_id=b.release_id ORDER BY t.id LIMIT 1), ''),
    COALESCE(m.year, (SELECT e.year FROM track t JOIN effective_track_metadata e ON e.track_id=t.id WHERE t.release_id=b.release_id AND e.year IS NOT NULL ORDER BY t.id LIMIT 1))
FROM album_backfill b LEFT JOIN release_application_metadata m ON m.release_id=b.release_id;
INSERT INTO album_artist_credit(album_id, position, artist_id, role, join_phrase)
SELECT b.album_id, c.position, c.artist_id, c.role, c.join_phrase
FROM album_backfill b JOIN release_artist_credit c ON c.release_id=b.release_id;
-- Preserve identities from the unshipped catalog prototype, without merging Albums.
INSERT INTO album_external_identity(album_id, provider, kind, external_id)
SELECT b.album_id, i.provider, i.kind, i.external_id FROM album_backfill b
JOIN release_external_identity i ON i.release_id=b.release_id
WHERE i.provider='musicbrainz' AND i.kind='release_group';
DELETE FROM release_external_identity WHERE provider='musicbrainz' AND kind='release_group';

CREATE TABLE release_new (
    id TEXT PRIMARY KEY,
    album_id TEXT NOT NULL REFERENCES album(id),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
INSERT INTO release_new(id, album_id, created_at)
SELECT r.id, b.album_id, r.created_at FROM release r JOIN album_backfill b ON b.release_id=r.id;
DROP TABLE release;
ALTER TABLE release_new RENAME TO release;
CREATE INDEX release_album ON release(album_id, id);
DROP TABLE album_backfill;
-- Fail and roll back if the rebuild violated any reference.
CREATE TEMP TABLE album_migration_integrity (valid INTEGER CHECK(valid = 1));
INSERT INTO album_migration_integrity SELECT 0 FROM pragma_foreign_key_check;
DROP TABLE album_migration_integrity;
PRAGMA user_version = 4;
COMMIT;
PRAGMA foreign_keys = ON;
