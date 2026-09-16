BEGIN;
-- Album-scoped catalog song evidence, never an exact-edition or Recording claim.
CREATE TABLE provider_track_association (
    track_id TEXT NOT NULL REFERENCES track(id) ON DELETE CASCADE,
    album_provider TEXT NOT NULL,
    album_kind TEXT NOT NULL,
    album_external_id TEXT NOT NULL,
    match_json TEXT NOT NULL CHECK(json_valid(match_json)),
    PRIMARY KEY(track_id, album_provider)
) STRICT;

-- Manual decisions coexist per provider. Preserve existing claims and ownership.
DROP TRIGGER manual_recording_claim_removed;
CREATE TEMP TABLE saved_manual_claim AS SELECT * FROM manual_track_recording_claim;
DROP TABLE manual_track_recording_claim;
ALTER TABLE manual_track_association RENAME TO old_manual_track_association;
CREATE TABLE manual_track_association (
    track_id TEXT NOT NULL REFERENCES track(id) ON DELETE CASCADE,
    album_provider TEXT NOT NULL,
    album_kind TEXT NOT NULL,
    album_external_id TEXT NOT NULL,
    candidate_json TEXT NOT NULL CHECK(json_valid(candidate_json)),
    PRIMARY KEY(track_id, album_provider)
) STRICT;
INSERT INTO manual_track_association SELECT * FROM old_manual_track_association;
CREATE TABLE manual_track_recording_claim (
    track_id TEXT NOT NULL,
    album_provider TEXT NOT NULL,
    recording_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY(track_id,album_provider,recording_id,provider,kind,external_id),
    FOREIGN KEY(track_id,album_provider) REFERENCES manual_track_association(track_id,album_provider) ON DELETE CASCADE,
    FOREIGN KEY(recording_id,provider,kind,external_id) REFERENCES recording_external_identity(recording_id,provider,kind,external_id) ON DELETE CASCADE
) STRICT;
INSERT INTO manual_track_recording_claim
SELECT c.track_id,m.album_provider,c.recording_id,c.provider,c.kind,c.external_id
FROM saved_manual_claim c JOIN manual_track_association m ON m.track_id=c.track_id;
DROP TABLE saved_manual_claim;
DROP TABLE old_manual_track_association;
CREATE INDEX manual_recording_claim_identity ON manual_track_recording_claim(recording_id,provider,kind,external_id);
CREATE TRIGGER manual_recording_claim_removed AFTER DELETE ON manual_track_recording_claim BEGIN
    DELETE FROM recording_external_identity
    WHERE recording_id=OLD.recording_id AND provider=OLD.provider AND kind=OLD.kind AND external_id=OLD.external_id
      AND EXISTS (SELECT 1 FROM recording_manual_identity m WHERE m.recording_id=OLD.recording_id
          AND m.provider=OLD.provider AND m.kind=OLD.kind AND m.external_id=OLD.external_id)
      AND NOT EXISTS (SELECT 1 FROM manual_track_recording_claim c WHERE c.recording_id=OLD.recording_id
          AND c.provider=OLD.provider AND c.kind=OLD.kind AND c.external_id=OLD.external_id);
END;
PRAGMA user_version=12;
COMMIT;
