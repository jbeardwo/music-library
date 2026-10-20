BEGIN;
-- Bounded Album-scoped topology observations; identity remains in existing tables.
CREATE TABLE IF NOT EXISTS musicbrainz_discovery (
 album_id TEXT PRIMARY KEY REFERENCES album(id) ON DELETE CASCADE,
 snapshot_json TEXT NOT NULL CHECK(json_valid(snapshot_json)),
 observed_at INTEGER NOT NULL DEFAULT(unixepoch())
) STRICT;
-- artist_id is the external MusicBrainz Artist MBID, not a canonical Artist ID.
CREATE TABLE IF NOT EXISTS musicbrainz_artist_page (
 artist_id TEXT NOT NULL, page_offset INTEGER NOT NULL,
 page_json TEXT NOT NULL CHECK(json_valid(page_json)),
 observed_at INTEGER NOT NULL,
 PRIMARY KEY(artist_id,page_offset)
) STRICT;
PRAGMA user_version=38;
COMMIT;
