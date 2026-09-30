BEGIN;
CREATE TABLE local_source_suppression (
    source_id TEXT PRIMARY KEY REFERENCES local_file_observation(source_id),
    suppressed_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
PRAGMA user_version = 18;
COMMIT;
