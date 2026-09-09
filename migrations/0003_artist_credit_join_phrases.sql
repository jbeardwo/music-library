BEGIN;
-- NULL retains the existing comma-separated display. Catalog credits specify
-- exact join phrases, including an empty phrase on the final credit.
ALTER TABLE track_artist_credit ADD COLUMN join_phrase TEXT;
ALTER TABLE release_artist_credit ADD COLUMN join_phrase TEXT;
PRAGMA user_version = 3;
COMMIT;
