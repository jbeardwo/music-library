BEGIN;
-- The local profile is an application preference owner, not a canonical entity.
CREATE TABLE hidden_artist_preference (
 profile_id TEXT NOT NULL DEFAULT 'local',
 artist_id TEXT NOT NULL REFERENCES artist(id) ON DELETE CASCADE,
 PRIMARY KEY(profile_id, artist_id)
) STRICT;
CREATE TABLE ignored_track_preference (
 profile_id TEXT NOT NULL DEFAULT 'local',
 track_id TEXT NOT NULL REFERENCES track(id) ON DELETE CASCADE,
 PRIMARY KEY(profile_id, track_id)
) STRICT;
PRAGMA user_version=32;
COMMIT;
