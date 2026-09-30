BEGIN;
-- Source-owned tags. Association and saved membership are queried separately.
ALTER TABLE file_metadata_observation ADD COLUMN genres_observed INTEGER NOT NULL DEFAULT 0 CHECK(genres_observed IN (0,1));
CREATE TABLE file_genre_observation (
    source_id TEXT NOT NULL REFERENCES playable_source(id) ON DELETE CASCADE,
    genre TEXT NOT NULL CHECK(length(trim(genre)) > 0),
    PRIMARY KEY(source_id, genre)
) STRICT;
CREATE INDEX file_genre_name ON file_genre_observation(genre, source_id);
PRAGMA user_version = 20;
COMMIT;
