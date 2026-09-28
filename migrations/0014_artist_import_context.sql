-- Supporting local import evidence, never a durable identity derived from paths.
CREATE TABLE local_artist_context (
    root_id TEXT NOT NULL REFERENCES discovery_root(id),
    directory BLOB NOT NULL,
    name_key TEXT NOT NULL,
    artist_id TEXT NOT NULL REFERENCES artist(id) ON DELETE CASCADE,
    PRIMARY KEY(root_id, directory, name_key, artist_id)
) STRICT;
CREATE INDEX local_artist_context_artist ON local_artist_context(artist_id);
