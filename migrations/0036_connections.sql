BEGIN;
-- Existing provider-keyed exclusions are retained verbatim, including Spotify choices.
CREATE INDEX IF NOT EXISTS connections_album_order ON album_browse_order(artist_key,title_key,album_id);
CREATE VIEW IF NOT EXISTS connected_local_track AS
SELECT s.track_id FROM track_source s CROSS JOIN local_file_observation l ON l.source_id=s.source_id
WHERE l.available=1 AND NOT EXISTS(SELECT 1 FROM local_source_suppression x WHERE x.source_id=s.source_id);
CREATE VIEW IF NOT EXISTS connected_musicbrainz_track AS
SELECT t.id AS track_id FROM track t WHERE EXISTS(
 SELECT 1 FROM recording_external_identity i WHERE i.recording_id=t.recording_id
 AND i.provider='musicbrainz' AND i.kind='recording' AND length(i.external_id)>0);
CREATE TRIGGER IF NOT EXISTS connections_local_attached AFTER INSERT ON track_source BEGIN
 DELETE FROM track_provider_exclusion WHERE track_id=NEW.track_id AND provider='local'
 AND EXISTS(SELECT 1 FROM connected_local_track WHERE track_id=NEW.track_id);
END;
CREATE TRIGGER IF NOT EXISTS connections_local_available AFTER UPDATE OF available ON local_file_observation WHEN NEW.available=1 BEGIN
 DELETE FROM track_provider_exclusion WHERE provider='local' AND track_id IN(SELECT track_id FROM track_source WHERE source_id=NEW.source_id)
 AND EXISTS(SELECT 1 FROM connected_local_track c WHERE c.track_id=track_provider_exclusion.track_id);
END;
CREATE TRIGGER IF NOT EXISTS connections_recording_attached AFTER INSERT ON recording_external_identity WHEN NEW.provider='musicbrainz' AND NEW.kind='recording' AND length(NEW.external_id)>0 BEGIN
 DELETE FROM track_provider_exclusion WHERE provider='musicbrainz' AND track_id IN(SELECT id FROM track WHERE recording_id=NEW.recording_id);
END;
PRAGMA user_version=36;
COMMIT;
