DROP TRIGGER IF EXISTS album_order_release_application_metadata_delete;
DROP TRIGGER IF EXISTS album_order_release_application_metadata_update;
DROP TRIGGER IF EXISTS album_order_release_application_metadata_insert;
DROP TRIGGER IF EXISTS album_order_effective_track_metadata_delete;
DROP TRIGGER IF EXISTS album_order_effective_track_metadata_update;
DROP TRIGGER IF EXISTS album_order_effective_track_metadata_insert;
DROP TRIGGER IF EXISTS album_order_metadata_insert;
DROP TRIGGER IF EXISTS album_order_metadata_update;
DROP TRIGGER IF EXISTS album_order_credit_insert;
DROP TRIGGER IF EXISTS album_order_credit_update;
DROP TRIGGER IF EXISTS album_order_credit_delete;
DROP TRIGGER IF EXISTS album_order_artist_name;
DROP TRIGGER IF EXISTS artwork_album_external_identity_insert;
DROP TRIGGER IF EXISTS artwork_album_external_identity_delete;
DROP TRIGGER IF EXISTS artwork_library_membership_insert;
DROP TRIGGER IF EXISTS artwork_library_membership_delete;
DROP TRIGGER IF EXISTS artwork_track_source_insert;
DROP TRIGGER IF EXISTS artwork_track_source_delete;
DROP TRIGGER IF EXISTS artwork_local_change;
DROP TABLE IF EXISTS album_browse_order;
DROP TABLE IF EXISTS album_artwork;
DROP INDEX IF EXISTS album_browse_year;

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
