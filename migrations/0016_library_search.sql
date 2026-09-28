BEGIN IMMEDIATE;
CREATE VIRTUAL TABLE artist_lookup USING fts5(name, content='artist', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2', prefix='2 3 4');
INSERT INTO artist_lookup(artist_lookup) VALUES ('rebuild');
CREATE TRIGGER artist_lookup_insert AFTER INSERT ON artist BEGIN
 INSERT INTO artist_lookup(rowid,name) VALUES (new.rowid,new.name);
END;
CREATE TRIGGER artist_lookup_delete AFTER DELETE ON artist BEGIN
 INSERT INTO artist_lookup(artist_lookup,rowid,name) VALUES ('delete',old.rowid,old.name);
END;
CREATE TRIGGER artist_lookup_update AFTER UPDATE OF name ON artist BEGIN
 INSERT INTO artist_lookup(artist_lookup,rowid,name) VALUES ('delete',old.rowid,old.name);
 INSERT INTO artist_lookup(rowid,name) VALUES (new.rowid,new.name);
END;
CREATE VIRTUAL TABLE album_lookup USING fts5(title, content='album_application_metadata', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2', prefix='2 3 4');
INSERT INTO album_lookup(album_lookup) VALUES ('rebuild');
CREATE TRIGGER album_lookup_insert AFTER INSERT ON album_application_metadata BEGIN
 INSERT INTO album_lookup(rowid,title) VALUES (new.rowid,new.title);
END;
CREATE TRIGGER album_lookup_delete AFTER DELETE ON album_application_metadata BEGIN
 INSERT INTO album_lookup(album_lookup,rowid,title) VALUES ('delete',old.rowid,old.title);
END;
CREATE TRIGGER album_lookup_update AFTER UPDATE OF title ON album_application_metadata BEGIN
 INSERT INTO album_lookup(album_lookup,rowid,title) VALUES ('delete',old.rowid,old.title);
 INSERT INTO album_lookup(rowid,title) VALUES (new.rowid,new.title);
END;
PRAGMA user_version=16;
COMMIT;
