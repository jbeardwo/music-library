BEGIN;

CREATE TABLE track_external_identity (
    track_id TEXT NOT NULL REFERENCES track(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY (track_id, provider, kind, external_id)
) STRICT;
CREATE INDEX track_external_identity_lookup
    ON track_external_identity(provider, kind, external_id);

CREATE TABLE release_external_identity (
    release_id TEXT NOT NULL REFERENCES release(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY (release_id, provider, kind, external_id)
) STRICT;
CREATE INDEX release_external_identity_lookup
    ON release_external_identity(provider, kind, external_id);

PRAGMA user_version = 2;
COMMIT;
