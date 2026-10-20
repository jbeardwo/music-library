BEGIN;
-- Provider observations are caches, never canonical identity or membership.
CREATE TABLE IF NOT EXISTS musicbrainz_match_state (
 album_id TEXT PRIMARY KEY REFERENCES album(id) ON DELETE CASCADE,
 run_id TEXT NOT NULL DEFAULT '', fingerprint TEXT NOT NULL, state TEXT NOT NULL, reason TEXT NOT NULL,
 track_reasons TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(track_reasons)),
 evaluation_version INTEGER NOT NULL, updated_at INTEGER NOT NULL DEFAULT(unixepoch())
) STRICT;
CREATE TABLE IF NOT EXISTS musicbrainz_candidate (
 album_id TEXT NOT NULL REFERENCES album(id) ON DELETE CASCADE,
 group_id TEXT NOT NULL, candidate_json TEXT NOT NULL CHECK(json_valid(candidate_json)),
 programs_json TEXT CHECK(programs_json IS NULL OR json_valid(programs_json)),
 inspection_json TEXT CHECK(inspection_json IS NULL OR json_valid(inspection_json)),
 inspection_observed_at INTEGER,
 origin TEXT NOT NULL CHECK(origin IN('automatic','manual','retained')),
 provider_rank INTEGER NOT NULL, reason TEXT NOT NULL,
 observed_at INTEGER NOT NULL DEFAULT(unixepoch()),
 program_observed_at INTEGER,
 PRIMARY KEY(album_id,group_id)
) STRICT;
CREATE INDEX IF NOT EXISTS musicbrainz_candidate_review ON musicbrainz_candidate(album_id,provider_rank,group_id);
-- No backfill: historical absence is not a completed empty provider search.
CREATE TRIGGER IF NOT EXISTS mb_track_metadata_changed AFTER UPDATE OF title,artist_names,disc_number,track_number ON effective_track_metadata
WHEN OLD.title IS NOT NEW.title OR OLD.artist_names IS NOT NEW.artist_names OR OLD.disc_number IS NOT NEW.disc_number OR OLD.track_number IS NOT NEW.track_number BEGIN
 UPDATE musicbrainz_match_state SET state='evidence_changed',reason='evidence_changed',track_reasons='{}' WHERE album_id=(SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id=NEW.track_id);
END;
CREATE TRIGGER IF NOT EXISTS mb_album_metadata_changed AFTER UPDATE ON album_application_metadata BEGIN
 UPDATE musicbrainz_match_state SET state='evidence_changed',reason='evidence_changed',track_reasons='{}' WHERE album_id=NEW.album_id;
END;
CREATE TRIGGER IF NOT EXISTS mb_album_override_changed AFTER INSERT ON album_metadata_override BEGIN
 UPDATE musicbrainz_match_state SET state='evidence_changed',reason='evidence_changed',track_reasons='{}' WHERE album_id=NEW.album_id;
END;
CREATE TRIGGER IF NOT EXISTS mb_album_override_updated AFTER UPDATE ON album_metadata_override BEGIN
 UPDATE musicbrainz_match_state SET state='evidence_changed',reason='evidence_changed',track_reasons='{}' WHERE album_id=NEW.album_id;
END;
CREATE TRIGGER IF NOT EXISTS mb_album_override_cleared AFTER DELETE ON album_metadata_override BEGIN
 UPDATE musicbrainz_match_state SET state='evidence_changed',reason='evidence_changed',track_reasons='{}' WHERE album_id=OLD.album_id;
END;
CREATE TRIGGER IF NOT EXISTS mb_membership_added AFTER INSERT ON library_membership BEGIN
 UPDATE musicbrainz_match_state SET state='evidence_changed',reason='evidence_changed',track_reasons='{}' WHERE album_id=(SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id=NEW.track_id);
END;
CREATE TRIGGER IF NOT EXISTS mb_membership_removed AFTER DELETE ON library_membership BEGIN
 UPDATE musicbrainz_match_state SET state='evidence_changed',reason='evidence_changed',track_reasons='{}' WHERE album_id=(SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id=OLD.track_id);
END;
CREATE TRIGGER IF NOT EXISTS mb_artist_identity_added AFTER INSERT ON artist_external_identity WHEN NEW.provider='musicbrainz' BEGIN
 UPDATE musicbrainz_match_state SET state='evidence_changed',reason='evidence_changed',track_reasons='{}' WHERE album_id IN(SELECT album_id FROM album_artist_credit WHERE artist_id=NEW.artist_id UNION SELECT r.album_id FROM track_artist_credit c JOIN track t ON t.id=c.track_id JOIN release r ON r.id=t.release_id WHERE c.artist_id=NEW.artist_id);
END;
CREATE TRIGGER IF NOT EXISTS mb_artist_identity_removed AFTER DELETE ON artist_external_identity WHEN OLD.provider='musicbrainz' BEGIN
 UPDATE musicbrainz_match_state SET state='evidence_changed',reason='evidence_changed',track_reasons='{}' WHERE album_id IN(SELECT album_id FROM album_artist_credit WHERE artist_id=OLD.artist_id UNION SELECT r.album_id FROM track_artist_credit c JOIN track t ON t.id=c.track_id JOIN release r ON r.id=t.release_id WHERE c.artist_id=OLD.artist_id);
END;
PRAGMA user_version=37;
COMMIT;
