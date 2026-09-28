-- Case-insensitive, stable keyset ordering for the desktop library panes.
BEGIN;
CREATE INDEX artist_browse_name ON artist(lower(name), id);
CREATE INDEX album_browse_title ON album_application_metadata(lower(title), album_id);
CREATE INDEX track_browse_title ON effective_track_metadata(lower(title), track_id);
PRAGMA user_version = 13;
COMMIT;
