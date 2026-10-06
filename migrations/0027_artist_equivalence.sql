BEGIN;
-- Explicit application identity links. Never rename Artists or rewrite credits.
CREATE TABLE artist_equivalence (
    artist_a TEXT NOT NULL REFERENCES artist(id) ON DELETE CASCADE,
    artist_b TEXT NOT NULL REFERENCES artist(id) ON DELETE CASCADE,
    confirmed_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(artist_a,artist_b),
    CHECK(artist_a<artist_b)
) STRICT;
CREATE INDEX artist_equivalence_reverse ON artist_equivalence(artist_b,artist_a);
PRAGMA user_version=27;
COMMIT;
