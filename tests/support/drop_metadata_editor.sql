DROP TABLE IF EXISTS spotify_discovery;
DROP TABLE IF EXISTS provider_cooldown;
-- Restore the pre-editor schema before testing an older migration.
DROP TRIGGER metadata_album_index_insert;
DROP TRIGGER metadata_album_index_update;
DROP TRIGGER metadata_album_index_delete;
DROP TRIGGER metadata_genre_insert;
DROP TRIGGER metadata_genre_delete;
DROP TRIGGER metadata_genre_attach;
DROP TRIGGER metadata_track_order_update;
DROP TRIGGER metadata_genre_detach;
DROP TRIGGER album_lookup_insert;
DROP TRIGGER album_lookup_update;
DROP TRIGGER album_lookup_delete;
DROP TABLE album_lookup;
DROP TRIGGER album_order_metadata_update;
CREATE TRIGGER IF NOT EXISTS album_order_metadata_update AFTER UPDATE OF title,year ON album_application_metadata BEGIN
 UPDATE album_browse_order SET title_key=lower(NEW.title) WHERE album_id=NEW.album_id;
 UPDATE album_browse_order SET (year,year_key)=(SELECT COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),printf('%012d',COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),999999999999)) FROM album_application_metadata a WHERE a.album_id=NEW.album_id) WHERE album_id=NEW.album_id;
END;
DROP TRIGGER album_order_credit_insert;
CREATE TRIGGER IF NOT EXISTS album_order_credit_insert AFTER INSERT ON album_artist_credit BEGIN
 UPDATE album_browse_order SET
 artist_key=COALESCE((SELECT lower(ar.name) FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),''),
 artist_id=COALESCE((SELECT artist_id FROM album_artist_credit c WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),'')
 WHERE album_id=NEW.album_id;
END;
DROP TRIGGER album_order_credit_update;
CREATE TRIGGER IF NOT EXISTS album_order_credit_update AFTER UPDATE ON album_artist_credit BEGIN
 UPDATE album_browse_order SET
 artist_key=COALESCE((SELECT lower(ar.name) FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),''),
 artist_id=COALESCE((SELECT artist_id FROM album_artist_credit c WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),'')
 WHERE album_id=NEW.album_id OR album_id=OLD.album_id;
END;
DROP TRIGGER album_order_credit_delete;
CREATE TRIGGER IF NOT EXISTS album_order_credit_delete AFTER DELETE ON album_artist_credit BEGIN
 UPDATE album_browse_order SET
 artist_key=COALESCE((SELECT lower(ar.name) FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),''),
 artist_id=COALESCE((SELECT artist_id FROM album_artist_credit c WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),'')
 WHERE album_id=OLD.album_id;
END;
DROP TRIGGER album_order_artist_name;
CREATE TRIGGER IF NOT EXISTS album_order_artist_name AFTER UPDATE OF name ON artist BEGIN
 UPDATE album_browse_order SET artist_key=lower(NEW.name) WHERE artist_id=NEW.id;
END;
DROP TRIGGER album_order_effective_track_metadata_insert;
CREATE TRIGGER IF NOT EXISTS album_order_effective_track_metadata_insert AFTER INSERT ON effective_track_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),printf('%012d',COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),999999999999)) FROM album_application_metadata a WHERE a.album_id=album_browse_order.album_id) WHERE album_id IN (SELECT r.album_id FROM release r JOIN track t ON t.release_id=r.id WHERE t.id=NEW.track_id);
END;
DROP TRIGGER album_order_effective_track_metadata_update;
CREATE TRIGGER IF NOT EXISTS album_order_effective_track_metadata_update AFTER UPDATE OF year ON effective_track_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),printf('%012d',COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),999999999999)) FROM album_application_metadata a WHERE a.album_id=album_browse_order.album_id) WHERE album_id IN (SELECT r.album_id FROM release r JOIN track t ON t.release_id=r.id WHERE t.id=NEW.track_id);
END;
DROP TRIGGER album_order_effective_track_metadata_delete;
CREATE TRIGGER IF NOT EXISTS album_order_effective_track_metadata_delete AFTER DELETE ON effective_track_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),printf('%012d',COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),999999999999)) FROM album_application_metadata a WHERE a.album_id=album_browse_order.album_id) WHERE album_id IN (SELECT r.album_id FROM release r JOIN track t ON t.release_id=r.id WHERE t.id=OLD.track_id);
END;
DROP TRIGGER album_order_release_application_metadata_insert;
CREATE TRIGGER IF NOT EXISTS album_order_release_application_metadata_insert AFTER INSERT ON release_application_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),printf('%012d',COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),999999999999)) FROM album_application_metadata a WHERE a.album_id=album_browse_order.album_id) WHERE album_id IN (SELECT album_id FROM release WHERE id=NEW.release_id);
END;
DROP TRIGGER album_order_release_application_metadata_update;
CREATE TRIGGER IF NOT EXISTS album_order_release_application_metadata_update AFTER UPDATE OF year ON release_application_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),printf('%012d',COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),999999999999)) FROM album_application_metadata a WHERE a.album_id=album_browse_order.album_id) WHERE album_id IN (SELECT album_id FROM release WHERE id=NEW.release_id);
END;
DROP TRIGGER album_order_release_application_metadata_delete;
CREATE TRIGGER IF NOT EXISTS album_order_release_application_metadata_delete AFTER DELETE ON release_application_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),printf('%012d',COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),999999999999)) FROM album_application_metadata a WHERE a.album_id=album_browse_order.album_id) WHERE album_id IN (SELECT album_id FROM release WHERE id=OLD.release_id);
END;
DROP VIEW effective_album_order_year;
DROP VIEW effective_album_metadata;
DROP TABLE album_metadata_override;
DROP TABLE track_metadata_override;
DROP TABLE effective_track_genre;
ALTER TABLE effective_track_metadata DROP COLUMN disc_number;
ALTER TABLE effective_track_metadata DROP COLUMN track_number;
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
PRAGMA user_version=29;
