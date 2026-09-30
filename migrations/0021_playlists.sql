BEGIN;
CREATE TABLE playlist (
 id TEXT PRIMARY KEY,
 name TEXT NOT NULL CHECK(length(trim(name)) > 0),
 created_at INTEGER NOT NULL DEFAULT(unixepoch()),
 updated_at INTEGER NOT NULL DEFAULT(unixepoch())
) STRICT;
CREATE INDEX playlist_name ON playlist(name COLLATE NOCASE, id);
CREATE TABLE playlist_entry (
 id TEXT PRIMARY KEY,
 playlist_id TEXT NOT NULL REFERENCES playlist(id) ON DELETE CASCADE,
 track_id TEXT NOT NULL REFERENCES track(id) ON DELETE RESTRICT,
 position INTEGER NOT NULL
) STRICT;
CREATE UNIQUE INDEX playlist_entry_order ON playlist_entry(playlist_id, position);
CREATE INDEX playlist_entry_track ON playlist_entry(track_id, playlist_id);
PRAGMA user_version=21;
COMMIT;
