BEGIN IMMEDIATE;
CREATE TABLE output_calibration (
    id INTEGER PRIMARY KEY CHECK (id=1),
    local_db REAL NOT NULL DEFAULT 0 CHECK (local_db BETWEEN -24 AND 0),
    spotify_db REAL NOT NULL DEFAULT 0 CHECK (spotify_db BETWEEN -24 AND 0)
) STRICT;
INSERT INTO output_calibration(id) VALUES (1);
PRAGMA user_version = 15;
COMMIT;
