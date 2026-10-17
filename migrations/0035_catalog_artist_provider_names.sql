CREATE TABLE IF NOT EXISTS catalog_artist_provider_name (
    catalog_artist_id TEXT NOT NULL,
    provider TEXT NOT NULL CHECK(provider='spotify'),
    kind TEXT NOT NULL CHECK(kind='artist'),
    external_id TEXT NOT NULL,
    name_key TEXT NOT NULL,
    PRIMARY KEY(catalog_artist_id,provider,kind,external_id)
) WITHOUT ROWID;
PRAGMA user_version=35;
CREATE INDEX IF NOT EXISTS trusted_artist_name_key ON trusted_artist_name(name_key,provider,external_id);
