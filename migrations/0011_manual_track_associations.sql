BEGIN;

-- User-confirmed Album-scoped association, not an exact edition claim. The typed
-- JSON snapshot retains provider presentation/occurrence evidence for offline use.
CREATE TABLE manual_track_association (
    track_id TEXT PRIMARY KEY REFERENCES track(id) ON DELETE CASCADE,
    album_provider TEXT NOT NULL,
    album_kind TEXT NOT NULL,
    album_external_id TEXT NOT NULL,
    candidate_json TEXT NOT NULL CHECK(json_valid(candidate_json))
) STRICT;

-- A marker means this canonical identity is supported solely by manual choices.
-- No marker means an independent path established/confirmed it.
CREATE TABLE recording_manual_identity (
    recording_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY(recording_id,provider,kind,external_id),
    FOREIGN KEY(recording_id,provider,kind,external_id)
        REFERENCES recording_external_identity(recording_id,provider,kind,external_id) ON DELETE CASCADE
) STRICT;
CREATE TABLE manual_track_recording_claim (
    track_id TEXT NOT NULL REFERENCES manual_track_association(track_id) ON DELETE CASCADE,
    recording_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY(track_id,recording_id,provider,kind,external_id),
    FOREIGN KEY(recording_id,provider,kind,external_id)
        REFERENCES recording_external_identity(recording_id,provider,kind,external_id) ON DELETE CASCADE
) STRICT;
CREATE INDEX manual_recording_claim_identity ON manual_track_recording_claim(recording_id,provider,kind,external_id);

CREATE TRIGGER recording_manual_confirmation BEFORE INSERT ON recording_external_identity BEGIN
    DELETE FROM recording_manual_identity WHERE recording_id=NEW.recording_id
        AND provider=NEW.provider AND kind=NEW.kind AND external_id=NEW.external_id;
END;
CREATE TRIGGER manual_recording_claim_removed AFTER DELETE ON manual_track_recording_claim BEGIN
    DELETE FROM recording_external_identity
    WHERE recording_id=OLD.recording_id AND provider=OLD.provider AND kind=OLD.kind AND external_id=OLD.external_id
      AND EXISTS (SELECT 1 FROM recording_manual_identity m WHERE m.recording_id=OLD.recording_id
          AND m.provider=OLD.provider AND m.kind=OLD.kind AND m.external_id=OLD.external_id)
      AND NOT EXISTS (SELECT 1 FROM manual_track_recording_claim c WHERE c.recording_id=OLD.recording_id
          AND c.provider=OLD.provider AND c.kind=OLD.kind AND c.external_id=OLD.external_id);
END;

PRAGMA user_version=11;
COMMIT;
