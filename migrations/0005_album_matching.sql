-- Reconstructible comparison keys, maintained alongside application Album metadata.
-- Rust backfills Unicode/whitespace normalization inside the migration transaction.
ALTER TABLE album_application_metadata ADD COLUMN match_title TEXT NOT NULL DEFAULT '';
ALTER TABLE album_application_metadata ADD COLUMN match_artist_credit TEXT NOT NULL DEFAULT '';
CREATE INDEX album_matching_lookup
    ON album_application_metadata(match_title, match_artist_credit, album_id);
