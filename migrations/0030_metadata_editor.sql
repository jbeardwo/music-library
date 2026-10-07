BEGIN IMMEDIATE;
-- NULL text means automatic; empty optional text explicitly blanks it.
-- Numeric *_set flags distinguish automatic from an explicit NULL/unknown value.
CREATE TABLE track_metadata_override (
 track_id TEXT PRIMARY KEY REFERENCES track(id) ON DELETE CASCADE,
 artist_credit TEXT CHECK(artist_credit IS NULL OR length(trim(artist_credit))>0), genre TEXT,
 year INTEGER CHECK(year BETWEEN 1 AND 9999), year_set INTEGER NOT NULL DEFAULT 0 CHECK(year_set IN(0,1)),
 disc_number INTEGER CHECK(disc_number BETWEEN 1 AND 99999), disc_number_set INTEGER NOT NULL DEFAULT 0 CHECK(disc_number_set IN(0,1)),
 track_number INTEGER CHECK(track_number BETWEEN 1 AND 99999), track_number_set INTEGER NOT NULL DEFAULT 0 CHECK(track_number_set IN(0,1)),
 updated_at INTEGER NOT NULL DEFAULT(unixepoch())
) STRICT;
CREATE TABLE album_metadata_override (
 album_id TEXT PRIMARY KEY REFERENCES album(id) ON DELETE CASCADE,
 title TEXT CHECK(title IS NULL OR length(trim(title))>0), artist_credit TEXT CHECK(artist_credit IS NULL OR length(trim(artist_credit))>0), genre TEXT, release_type TEXT,
 year INTEGER CHECK(year BETWEEN 1 AND 9999), year_set INTEGER NOT NULL DEFAULT 0 CHECK(year_set IN(0,1)),
 updated_at INTEGER NOT NULL DEFAULT(unixepoch())
) STRICT;
CREATE VIEW effective_album_metadata AS
SELECT a.rowid AS rowid,a.album_id,COALESCE(o.title,a.title) AS title,
 CASE WHEN o.year_set THEN o.year ELSE a.year END AS year,
 a.match_title,a.match_artist_credit,o.artist_credit,o.genre,o.release_type,COALESCE(o.year_set,0) AS year_overridden
FROM album_application_metadata a LEFT JOIN album_metadata_override o ON o.album_id=a.album_id;
CREATE VIEW effective_album_order_year AS
SELECT a.album_id,CASE WHEN a.year_overridden THEN a.year ELSE COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)) END AS year FROM effective_album_metadata a;
ALTER TABLE effective_track_metadata ADD COLUMN disc_number INTEGER;
ALTER TABLE effective_track_metadata ADD COLUMN track_number INTEGER;
UPDATE effective_track_metadata SET disc_number=(SELECT disc_number FROM track WHERE id=track_id),track_number=(SELECT track_number FROM track WHERE id=track_id);
CREATE TABLE effective_track_genre (
 track_id TEXT NOT NULL REFERENCES track(id) ON DELETE CASCADE,genre TEXT NOT NULL,
 PRIMARY KEY(track_id,genre)
) STRICT;
CREATE INDEX effective_genre_lookup ON effective_track_genre(genre,track_id);
INSERT OR IGNORE INTO effective_track_genre SELECT ts.track_id,g.genre FROM track_source ts JOIN file_genre_observation g ON g.source_id=ts.source_id;
-- Source/association writers also maintain the indexed automatic Genre projection.
CREATE TRIGGER metadata_genre_insert AFTER INSERT ON file_genre_observation BEGIN
 INSERT OR IGNORE INTO effective_track_genre SELECT ts.track_id,new.genre FROM track_source ts JOIN track t ON t.id=ts.track_id JOIN release r ON r.id=t.release_id LEFT JOIN track_metadata_override o ON o.track_id=t.id LEFT JOIN album_metadata_override a ON a.album_id=r.album_id WHERE ts.source_id=new.source_id AND o.genre IS NULL AND a.genre IS NULL;
END;
CREATE TRIGGER metadata_genre_delete AFTER DELETE ON file_genre_observation BEGIN
 DELETE FROM effective_track_genre WHERE genre=old.genre AND track_id IN(SELECT track_id FROM track_source WHERE source_id=old.source_id) AND NOT EXISTS(SELECT 1 FROM track_metadata_override o WHERE o.track_id=effective_track_genre.track_id AND o.genre IS NOT NULL) AND NOT EXISTS(SELECT 1 FROM track t JOIN release r ON r.id=t.release_id JOIN album_metadata_override a ON a.album_id=r.album_id WHERE t.id=effective_track_genre.track_id AND a.genre IS NOT NULL) AND NOT EXISTS(SELECT 1 FROM track_source ts JOIN file_genre_observation g ON g.source_id=ts.source_id WHERE ts.track_id=effective_track_genre.track_id AND g.genre=old.genre);
END;
CREATE TRIGGER metadata_genre_attach AFTER INSERT ON track_source BEGIN
 INSERT OR IGNORE INTO effective_track_genre SELECT new.track_id,g.genre FROM file_genre_observation g JOIN track t ON t.id=new.track_id JOIN release r ON r.id=t.release_id LEFT JOIN track_metadata_override o ON o.track_id=t.id LEFT JOIN album_metadata_override a ON a.album_id=r.album_id WHERE g.source_id=new.source_id AND o.genre IS NULL AND a.genre IS NULL;
END;
CREATE TRIGGER metadata_genre_detach AFTER DELETE ON track_source BEGIN
 DELETE FROM effective_track_genre WHERE track_id=old.track_id AND NOT EXISTS(SELECT 1 FROM track_metadata_override o WHERE o.track_id=old.track_id AND o.genre IS NOT NULL) AND NOT EXISTS(SELECT 1 FROM track t JOIN release r ON r.id=t.release_id JOIN album_metadata_override a ON a.album_id=r.album_id WHERE t.id=old.track_id AND a.genre IS NOT NULL) AND NOT EXISTS(SELECT 1 FROM track_source ts JOIN file_genre_observation g ON g.source_id=ts.source_id WHERE ts.track_id=old.track_id AND g.genre=effective_track_genre.genre);
END;
-- External-content FTS reads the same effective title that is indexed.
DROP TRIGGER album_lookup_insert;
DROP TRIGGER album_lookup_update;
DROP TRIGGER album_lookup_delete;
DROP TABLE album_lookup;
CREATE VIRTUAL TABLE album_lookup USING fts5(title, content='effective_album_metadata', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2', prefix='2 3 4');
INSERT INTO album_lookup(album_lookup) VALUES('rebuild');
CREATE TRIGGER album_lookup_insert AFTER INSERT ON album_application_metadata BEGIN
 INSERT INTO album_lookup(rowid,title) VALUES(new.rowid,COALESCE((SELECT title FROM album_metadata_override WHERE album_id=new.album_id),new.title));
END;
CREATE TRIGGER album_lookup_update AFTER UPDATE OF title ON album_application_metadata BEGIN
 INSERT INTO album_lookup(album_lookup,rowid,title) VALUES('delete',old.rowid,COALESCE((SELECT title FROM album_metadata_override WHERE album_id=old.album_id),old.title));
 INSERT INTO album_lookup(rowid,title) VALUES(new.rowid,COALESCE((SELECT title FROM album_metadata_override WHERE album_id=new.album_id),new.title));
END;
CREATE TRIGGER album_lookup_delete BEFORE DELETE ON album_application_metadata BEGIN
 INSERT INTO album_lookup(album_lookup,rowid,title) VALUES('delete',old.rowid,COALESCE((SELECT title FROM album_metadata_override WHERE album_id=old.album_id),old.title));
END;
CREATE TRIGGER metadata_album_index_insert AFTER INSERT ON album_metadata_override BEGIN
 INSERT INTO album_lookup(album_lookup,rowid,title) SELECT 'delete',a.rowid,a.title FROM album_application_metadata a WHERE a.album_id=new.album_id;
 INSERT INTO album_lookup(rowid,title) SELECT a.rowid,COALESCE(new.title,a.title) FROM album_application_metadata a WHERE a.album_id=new.album_id;
 UPDATE album_browse_order SET title_key=lower((SELECT title FROM effective_album_metadata WHERE album_id=new.album_id)),
 year=(SELECT year FROM effective_album_order_year WHERE album_id=new.album_id),
 year_key=printf('%012d',COALESCE(999999-(SELECT year FROM effective_album_order_year WHERE album_id=new.album_id),999999999999)),
 artist_key=COALESCE((SELECT lower(artist_credit) FROM album_metadata_override WHERE album_id=new.album_id),(SELECT lower(ar.name) FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=new.album_id ORDER BY position LIMIT 1),'')
 WHERE album_id=new.album_id;
END;
CREATE TRIGGER metadata_album_index_update AFTER UPDATE ON album_metadata_override BEGIN
 INSERT INTO album_lookup(album_lookup,rowid,title) SELECT 'delete',a.rowid,COALESCE(old.title,a.title) FROM album_application_metadata a WHERE a.album_id=new.album_id;
 INSERT INTO album_lookup(rowid,title) SELECT a.rowid,COALESCE(new.title,a.title) FROM album_application_metadata a WHERE a.album_id=new.album_id;
 UPDATE album_browse_order SET title_key=lower((SELECT title FROM effective_album_metadata WHERE album_id=new.album_id)),
 year=(SELECT year FROM effective_album_order_year WHERE album_id=new.album_id),
 year_key=printf('%012d',COALESCE(999999-(SELECT year FROM effective_album_order_year WHERE album_id=new.album_id),999999999999)),
 artist_key=COALESCE((SELECT lower(artist_credit) FROM album_metadata_override WHERE album_id=new.album_id),(SELECT lower(ar.name) FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=new.album_id ORDER BY position LIMIT 1),'')
 WHERE album_id=new.album_id;
END;
CREATE TRIGGER metadata_album_index_delete AFTER DELETE ON album_metadata_override BEGIN
 INSERT INTO album_lookup(album_lookup,rowid,title) SELECT 'delete',a.rowid,COALESCE(old.title,a.title) FROM album_application_metadata a WHERE a.album_id=old.album_id;
 INSERT INTO album_lookup(rowid,title) SELECT a.rowid,a.title FROM album_application_metadata a WHERE a.album_id=old.album_id;
 UPDATE album_browse_order SET title_key=lower((SELECT title FROM effective_album_metadata WHERE album_id=old.album_id)),
 year=(SELECT year FROM effective_album_order_year WHERE album_id=old.album_id),
 year_key=printf('%012d',COALESCE(999999-(SELECT year FROM effective_album_order_year WHERE album_id=old.album_id),999999999999)),
 artist_key=COALESCE((SELECT lower(artist_credit) FROM album_metadata_override WHERE album_id=old.album_id),(SELECT lower(ar.name) FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=old.album_id ORDER BY position LIMIT 1),'')
 WHERE album_id=old.album_id;
END;
DROP TRIGGER album_order_metadata_update;
CREATE TRIGGER album_order_metadata_update AFTER UPDATE OF title,year ON album_application_metadata BEGIN
 UPDATE album_browse_order SET title_key=lower((SELECT title FROM effective_album_metadata WHERE album_id=new.album_id)),
 year=(SELECT year FROM effective_album_order_year WHERE album_id=new.album_id),
 year_key=printf('%012d',COALESCE(999999-(SELECT year FROM effective_album_order_year WHERE album_id=new.album_id),999999999999)) WHERE album_id=new.album_id;
END;
DROP TRIGGER album_order_effective_track_metadata_insert;
CREATE TRIGGER album_order_effective_track_metadata_insert AFTER INSERT ON effective_track_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT y.year,printf('%012d',COALESCE(999999-y.year,999999999999)) FROM effective_album_order_year y WHERE y.album_id=album_browse_order.album_id) WHERE album_id IN(SELECT r.album_id FROM release r JOIN track t ON t.release_id=r.id WHERE t.id=NEW.track_id);
END;
DROP TRIGGER album_order_effective_track_metadata_update;
CREATE TRIGGER album_order_effective_track_metadata_update AFTER UPDATE OF year ON effective_track_metadata WHEN new.year IS NOT old.year BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT y.year,printf('%012d',COALESCE(999999-y.year,999999999999)) FROM effective_album_order_year y WHERE y.album_id=album_browse_order.album_id) WHERE album_id IN(SELECT r.album_id FROM release r JOIN track t ON t.release_id=r.id WHERE t.id=NEW.track_id);
END;
DROP TRIGGER album_order_effective_track_metadata_delete;
CREATE TRIGGER album_order_effective_track_metadata_delete AFTER DELETE ON effective_track_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT y.year,printf('%012d',COALESCE(999999-y.year,999999999999)) FROM effective_album_order_year y WHERE y.album_id=album_browse_order.album_id) WHERE album_id IN(SELECT r.album_id FROM release r JOIN track t ON t.release_id=r.id WHERE t.id=OLD.track_id);
END;
DROP TRIGGER album_order_release_application_metadata_insert;
CREATE TRIGGER album_order_release_application_metadata_insert AFTER INSERT ON release_application_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT y.year,printf('%012d',COALESCE(999999-y.year,999999999999)) FROM effective_album_order_year y WHERE y.album_id=album_browse_order.album_id) WHERE album_id IN(SELECT album_id FROM release WHERE id=NEW.release_id);
END;
DROP TRIGGER album_order_release_application_metadata_update;
CREATE TRIGGER album_order_release_application_metadata_update AFTER UPDATE OF year ON release_application_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT y.year,printf('%012d',COALESCE(999999-y.year,999999999999)) FROM effective_album_order_year y WHERE y.album_id=album_browse_order.album_id) WHERE album_id IN(SELECT album_id FROM release WHERE id=NEW.release_id);
END;
DROP TRIGGER album_order_release_application_metadata_delete;
CREATE TRIGGER album_order_release_application_metadata_delete AFTER DELETE ON release_application_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT y.year,printf('%012d',COALESCE(999999-y.year,999999999999)) FROM effective_album_order_year y WHERE y.album_id=album_browse_order.album_id) WHERE album_id IN(SELECT album_id FROM release WHERE id=OLD.release_id);
END;
DROP TRIGGER album_order_credit_insert;
CREATE TRIGGER album_order_credit_insert AFTER INSERT ON album_artist_credit BEGIN
 UPDATE album_browse_order SET
 artist_key=COALESCE((SELECT lower(artist_credit) FROM album_metadata_override WHERE album_id=album_browse_order.album_id),(SELECT lower(ar.name) FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),''),
 artist_id=COALESCE((SELECT artist_id FROM album_artist_credit c WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),'')
 WHERE album_id=NEW.album_id;
END;
DROP TRIGGER album_order_credit_update;
CREATE TRIGGER album_order_credit_update AFTER UPDATE ON album_artist_credit BEGIN
 UPDATE album_browse_order SET
 artist_key=COALESCE((SELECT lower(artist_credit) FROM album_metadata_override WHERE album_id=album_browse_order.album_id),(SELECT lower(ar.name) FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),''),
 artist_id=COALESCE((SELECT artist_id FROM album_artist_credit c WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),'')
 WHERE album_id=NEW.album_id OR album_id=OLD.album_id;
END;
DROP TRIGGER album_order_credit_delete;
CREATE TRIGGER album_order_credit_delete AFTER DELETE ON album_artist_credit BEGIN
 UPDATE album_browse_order SET
 artist_key=COALESCE((SELECT lower(artist_credit) FROM album_metadata_override WHERE album_id=album_browse_order.album_id),(SELECT lower(ar.name) FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),''),
 artist_id=COALESCE((SELECT artist_id FROM album_artist_credit c WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),'')
 WHERE album_id=OLD.album_id;
END;
DROP TRIGGER album_order_artist_name;
CREATE TRIGGER album_order_artist_name AFTER UPDATE OF name ON artist BEGIN
 UPDATE album_browse_order SET artist_key=lower(NEW.name) WHERE artist_id=NEW.id;
END;
CREATE TRIGGER metadata_track_order_update AFTER UPDATE OF disc_number,track_number ON track BEGIN
 UPDATE effective_track_metadata SET disc_number=CASE WHEN (SELECT disc_number_set FROM track_metadata_override WHERE track_id=new.id)=1 THEN (SELECT disc_number FROM track_metadata_override WHERE track_id=new.id) ELSE new.disc_number END,
 track_number=CASE WHEN (SELECT track_number_set FROM track_metadata_override WHERE track_id=new.id)=1 THEN (SELECT track_number FROM track_metadata_override WHERE track_id=new.id) ELSE new.track_number END WHERE track_id=new.id;
END;
PRAGMA user_version=30;
COMMIT;
