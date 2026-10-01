BEGIN;
-- Source-owned duration evidence; canonical Track existence is the only parent.
CREATE TABLE track_duration_observation (
    track_id TEXT NOT NULL REFERENCES track(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    source_key TEXT NOT NULL,
    duration_ms INTEGER NOT NULL CHECK(duration_ms>=0),
    quality INTEGER NOT NULL CHECK(quality BETWEEN 0 AND 2),
    PRIMARY KEY(track_id,provider,source_key)
) STRICT;
CREATE INDEX track_duration_choice ON track_duration_observation(track_id,quality,provider,source_key);
ALTER TABLE effective_track_metadata ADD COLUMN duration_approximate INTEGER NOT NULL DEFAULT 0 CHECK(duration_approximate IN (0,1));
ALTER TABLE effective_track_metadata ADD COLUMN genre_names TEXT NOT NULL DEFAULT '';
-- Offline backfill where accepted manual evidence already retained a length.
INSERT INTO track_duration_observation
SELECT track_id,album_provider,album_external_id,json_extract(candidate_json,'$.evidence.duration_ms'),
       CASE WHEN album_provider='musicbrainz' THEN 2 ELSE 0 END
FROM manual_track_association WHERE json_type(candidate_json,'$.evidence.duration_ms')='integer'
AND json_extract(candidate_json,'$.evidence.duration_ms')>=0;
UPDATE effective_track_metadata SET
 duration_ms=COALESCE(duration_ms,(SELECT duration_ms FROM track_duration_observation d WHERE d.track_id=effective_track_metadata.track_id ORDER BY quality,provider,source_key LIMIT 1)),
 duration_approximate=CASE WHEN duration_ms IS NULL THEN COALESCE((SELECT quality=2 FROM track_duration_observation d WHERE d.track_id=effective_track_metadata.track_id ORDER BY quality,provider,source_key LIMIT 1),0) ELSE 0 END,
 genre_names=COALESCE((SELECT group_concat(genre,' · ') FROM (SELECT DISTINCT g.genre FROM track_source s JOIN file_genre_observation g ON g.source_id=s.source_id WHERE s.track_id=effective_track_metadata.track_id ORDER BY g.genre COLLATE NOCASE,g.genre)),'');
CREATE INDEX song_details_artist ON effective_track_metadata(lower(artist_names),track_id);
CREATE INDEX song_details_album ON effective_track_metadata(lower(release_title),track_id);
CREATE INDEX song_details_genre ON effective_track_metadata(lower(genre_names),track_id);
PRAGMA user_version=22;
COMMIT;
