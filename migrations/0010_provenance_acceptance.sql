BEGIN;

-- Absence of a marker means independently established. Existing associations
-- therefore remain protected; migration never promotes historical observations.
CREATE TABLE album_provenance_identity (
    album_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY (album_id, provider, kind, external_id),
    FOREIGN KEY (album_id, provider, kind, external_id)
        REFERENCES album_external_identity(album_id, provider, kind, external_id) ON DELETE CASCADE
) STRICT;
CREATE TABLE recording_provenance_identity (
    recording_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY (recording_id, provider, kind, external_id),
    FOREIGN KEY (recording_id, provider, kind, external_id)
        REFERENCES recording_external_identity(recording_id, provider, kind, external_id) ON DELETE CASCADE
) STRICT;

-- An independent INSERT confirms an association even with ON CONFLICT DO NOTHING.
-- Automatic acceptance inserts its marker only after creating a NEW association.
CREATE TRIGGER album_identity_confirmation BEFORE INSERT ON album_external_identity BEGIN
    DELETE FROM album_provenance_identity WHERE album_id=NEW.album_id
        AND provider=NEW.provider AND kind=NEW.kind AND external_id=NEW.external_id;
END;
CREATE TRIGGER recording_identity_confirmation BEFORE INSERT ON recording_external_identity BEGIN
    DELETE FROM recording_provenance_identity WHERE recording_id=NEW.recording_id
        AND provider=NEW.provider AND kind=NEW.kind AND external_id=NEW.external_id;
END;

PRAGMA user_version = 10;
COMMIT;
