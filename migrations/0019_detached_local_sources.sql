-- Store owns the transaction with foreign keys temporarily disabled for rebuild.
CREATE TABLE local_file_observation_new (
    source_id TEXT PRIMARY KEY REFERENCES playable_source(id) ON DELETE CASCADE,
    root_id TEXT REFERENCES discovery_root(id) ON DELETE SET NULL,
    path BLOB NOT NULL,
    size_bytes INTEGER NOT NULL,
    modified_ns INTEGER NOT NULL,
    available INTEGER NOT NULL CHECK (available IN (0, 1)),
    last_seen_scan_id INTEGER REFERENCES scan_run(id) ON DELETE SET NULL,
    last_observed_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (root_id, path)
) STRICT;
INSERT INTO local_file_observation_new SELECT * FROM local_file_observation;
DROP TABLE local_file_observation;
ALTER TABLE local_file_observation_new RENAME TO local_file_observation;
CREATE INDEX local_file_root_seen ON local_file_observation(root_id, last_seen_scan_id);
CREATE INDEX local_file_available ON local_file_observation(available, source_id);
CREATE INDEX local_file_path ON local_file_observation(path, source_id);
-- A source may be encountered by overlapping locations without duplicating its identity.
CREATE TABLE local_root_source (
    root_id TEXT NOT NULL REFERENCES discovery_root(id) ON DELETE CASCADE,
    source_id TEXT NOT NULL REFERENCES local_file_observation(source_id) ON DELETE CASCADE,
    last_seen_scan_id INTEGER REFERENCES scan_run(id) ON DELETE SET NULL,
    PRIMARY KEY(root_id, source_id)
) STRICT;
CREATE INDEX local_root_source_source ON local_root_source(source_id, root_id);
INSERT INTO local_root_source(root_id,source_id,last_seen_scan_id)
SELECT root_id,source_id,last_seen_scan_id FROM local_file_observation WHERE root_id IS NOT NULL;
CREATE TRIGGER artwork_local_change AFTER UPDATE OF size_bytes,modified_ns,path,available ON local_file_observation
WHEN OLD.size_bytes<>NEW.size_bytes OR OLD.modified_ns<>NEW.modified_ns OR OLD.path<>NEW.path OR OLD.available<>NEW.available BEGIN
 UPDATE album_artwork SET checked_at=0 WHERE album_id IN (SELECT r.album_id FROM track_source ts JOIN track t ON t.id=ts.track_id JOIN release r ON r.id=t.release_id WHERE ts.source_id=NEW.source_id);
END;
CREATE TEMP TABLE local_migration_integrity(valid INTEGER CHECK(valid=1));
INSERT INTO local_migration_integrity SELECT 0 FROM pragma_foreign_key_check;
DROP TABLE local_migration_integrity;
PRAGMA user_version = 19;
