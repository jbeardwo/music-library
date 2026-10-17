CREATE TABLE IF NOT EXISTS trusted_artist_name (
    provider TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind='artist'),
    external_id TEXT NOT NULL,
    name TEXT NOT NULL,
    name_key TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('preferred','alias')),
    PRIMARY KEY(provider,kind,external_id,name_key)
) WITHOUT ROWID;
PRAGMA user_version=34;
