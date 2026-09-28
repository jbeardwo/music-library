DROP TRIGGER IF EXISTS artist_lookup_insert;
DROP TRIGGER IF EXISTS artist_lookup_update;
DROP TRIGGER IF EXISTS artist_lookup_delete;
DROP TRIGGER IF EXISTS album_lookup_insert;
DROP TRIGGER IF EXISTS album_lookup_update;
DROP TRIGGER IF EXISTS album_lookup_delete;
DROP TABLE IF EXISTS artist_lookup;
DROP TABLE IF EXISTS album_lookup;
DROP TABLE IF EXISTS output_calibration;
DROP TRIGGER recording_manual_confirmation;
DROP TRIGGER manual_recording_claim_removed;
DROP TABLE manual_track_recording_claim;
DROP TABLE recording_manual_identity;
DROP TABLE manual_track_association;
DROP TABLE IF EXISTS provider_track_association;

-- Downgrade fixtures also remove the later library browsing indexes.
DROP INDEX IF EXISTS artist_browse_name;
DROP INDEX IF EXISTS album_browse_title;
DROP INDEX IF EXISTS track_browse_title;

DROP TABLE IF EXISTS local_artist_context;
