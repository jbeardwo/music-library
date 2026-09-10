BEGIN;
CREATE TABLE artist_external_identity (
    artist_id TEXT NOT NULL REFERENCES artist(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY (artist_id, provider, kind, external_id)
) STRICT;
CREATE INDEX artist_external_identity_lookup
    ON artist_external_identity(provider, kind, external_id);
PRAGMA user_version = 6;
COMMIT;
