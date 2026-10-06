BEGIN;
-- Summaries are local review state, not matching evidence or Library membership.
CREATE TABLE spotify_connection_review (
    track_id TEXT PRIMARY KEY REFERENCES track(id) ON DELETE CASCADE,
    reason_code TEXT NOT NULL DEFAULT 'not_evaluated',
    reason TEXT NOT NULL DEFAULT 'No reconciliation attempted yet'
) STRICT;
INSERT INTO spotify_connection_review(track_id) SELECT id FROM track;
CREATE INDEX spotify_connection_reason ON spotify_connection_review(lower(reason),track_id);
CREATE TRIGGER spotify_review_track_added AFTER INSERT ON track BEGIN
    INSERT INTO spotify_connection_review(track_id) VALUES(NEW.id);
END;
CREATE INDEX spotify_review_manual ON manual_track_association(album_provider,track_id);
CREATE INDEX spotify_review_automatic ON provider_track_association(album_provider,track_id);
-- Mirror track_provider_occurrences precedence: manual, durable Track identity,
-- then an accepted occurrence under the current trusted Album representation.
CREATE VIEW trusted_spotify_track AS
SELECT m.track_id FROM manual_track_association m, json_each(m.candidate_json,'$.evidence.identities') j
WHERE m.album_provider='spotify' AND json_extract(j.value,'$.provider')='spotify'
AND json_extract(j.value,'$.kind')='track' AND length(json_extract(j.value,'$.external_id'))>0
UNION ALL
SELECT i.track_id FROM track_external_identity i WHERE i.provider='spotify' AND i.kind='track' AND length(i.external_id)>0
AND NOT EXISTS(SELECT 1 FROM manual_track_association m WHERE m.track_id=i.track_id AND m.album_provider='spotify')
UNION ALL
SELECT p.track_id FROM provider_track_association p JOIN track t ON t.id=p.track_id
JOIN release r ON r.id=t.release_id JOIN album_external_identity a ON a.album_id=r.album_id
AND a.provider=p.album_provider AND a.kind=p.album_kind AND a.external_id=p.album_external_id,
json_each(p.match_json,'$.occurrences') j
WHERE p.album_provider='spotify' AND json_extract(j.value,'$.provider')='spotify'
AND json_extract(j.value,'$.kind')='track' AND length(json_extract(j.value,'$.external_id'))>0
AND NOT EXISTS(SELECT 1 FROM manual_track_association m WHERE m.track_id=p.track_id AND m.album_provider='spotify')
AND NOT EXISTS(SELECT 1 FROM track_external_identity i WHERE i.track_id=p.track_id AND i.provider='spotify');
PRAGMA user_version=26;
COMMIT;
