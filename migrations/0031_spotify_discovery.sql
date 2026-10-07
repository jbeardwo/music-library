BEGIN;
-- Bounded catalog responses, independent of evaluator decisions and user exclusions.
CREATE TABLE spotify_discovery (
 fingerprint TEXT PRIMARY KEY,
 response_json TEXT NOT NULL CHECK(json_valid(response_json)),
 completed_at INTEGER NOT NULL
) STRICT;
CREATE INDEX spotify_discovery_age ON spotify_discovery(completed_at);
CREATE TABLE provider_cooldown (
 provider TEXT PRIMARY KEY,
 until_epoch INTEGER NOT NULL
) STRICT;
PRAGMA user_version=31;
COMMIT;
