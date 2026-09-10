-- Presentation belongs to each ordered credit, not the referenced Artist identity.
-- NULL supports older/raw fixture writers; reassignment freezes that fallback first.
BEGIN;
ALTER TABLE album_artist_credit ADD COLUMN credited_name TEXT;
ALTER TABLE release_artist_credit ADD COLUMN credited_name TEXT;
ALTER TABLE track_artist_credit ADD COLUMN credited_name TEXT;
UPDATE album_artist_credit SET credited_name = (SELECT name FROM artist WHERE id = artist_id);
UPDATE release_artist_credit SET credited_name = (SELECT name FROM artist WHERE id = artist_id);
UPDATE track_artist_credit SET credited_name = (SELECT name FROM artist WHERE id = artist_id);
PRAGMA user_version = 7;
COMMIT;
