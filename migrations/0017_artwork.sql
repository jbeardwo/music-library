BEGIN;
-- Reconstructible files live outside SQLite; provenance survives cache eviction.
CREATE TABLE IF NOT EXISTS album_artwork (
    album_id TEXT PRIMARY KEY REFERENCES album(id) ON DELETE CASCADE,
    origin TEXT NOT NULL,
    locator TEXT NOT NULL,
    cache_name TEXT NOT NULL,
    checked_at INTEGER NOT NULL
) STRICT;
-- Materialized ordering keys keep grouped browsing independent of credit scans.
CREATE TABLE IF NOT EXISTS album_browse_order (
    album_id TEXT PRIMARY KEY REFERENCES album(id) ON DELETE CASCADE,
    year INTEGER,
    title_key TEXT NOT NULL,
    year_key TEXT NOT NULL,
    artist_key TEXT NOT NULL,
    artist_id TEXT NOT NULL
) STRICT;
INSERT OR REPLACE INTO album_browse_order
SELECT a.album_id,COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),lower(a.title),printf('%012d', COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)), 999999999999)),
 COALESCE((SELECT lower(ar.name) FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=a.album_id ORDER BY c.position LIMIT 1),''),
 COALESCE((SELECT artist_id FROM album_artist_credit c WHERE c.album_id=a.album_id ORDER BY c.position LIMIT 1),'')
FROM album_application_metadata a;
CREATE INDEX IF NOT EXISTS album_order_artist_identity ON album_browse_order(artist_id,album_id);
CREATE INDEX IF NOT EXISTS album_order_title ON album_browse_order(title_key,album_id);
CREATE INDEX IF NOT EXISTS album_order_year ON album_browse_order(year_key,title_key,album_id);
CREATE INDEX IF NOT EXISTS album_order_artist ON album_browse_order(artist_key,artist_id,year_key || char(31) || title_key,album_id);
CREATE TRIGGER IF NOT EXISTS album_order_metadata_insert AFTER INSERT ON album_application_metadata BEGIN
 INSERT INTO album_browse_order VALUES (NEW.album_id,NEW.year,lower(NEW.title),printf('%012d',COALESCE(999999-NEW.year,999999999999)),'','');
END;
CREATE TRIGGER IF NOT EXISTS album_order_metadata_update AFTER UPDATE OF title,year ON album_application_metadata BEGIN
 UPDATE album_browse_order SET title_key=lower(NEW.title) WHERE album_id=NEW.album_id;
 UPDATE album_browse_order SET (year,year_key)=(SELECT COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),printf('%012d',COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),999999999999)) FROM album_application_metadata a WHERE a.album_id=NEW.album_id) WHERE album_id=NEW.album_id;
END;
CREATE TRIGGER IF NOT EXISTS album_order_credit_insert AFTER INSERT ON album_artist_credit BEGIN
 UPDATE album_browse_order SET
 artist_key=COALESCE((SELECT lower(ar.name) FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),''),
 artist_id=COALESCE((SELECT artist_id FROM album_artist_credit c WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),'')
 WHERE album_id=NEW.album_id;
END;
CREATE TRIGGER IF NOT EXISTS album_order_credit_update AFTER UPDATE ON album_artist_credit BEGIN
 UPDATE album_browse_order SET
 artist_key=COALESCE((SELECT lower(ar.name) FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),''),
 artist_id=COALESCE((SELECT artist_id FROM album_artist_credit c WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),'')
 WHERE album_id=NEW.album_id OR album_id=OLD.album_id;
END;
CREATE TRIGGER IF NOT EXISTS album_order_credit_delete AFTER DELETE ON album_artist_credit BEGIN
 UPDATE album_browse_order SET
 artist_key=COALESCE((SELECT lower(ar.name) FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),''),
 artist_id=COALESCE((SELECT artist_id FROM album_artist_credit c WHERE c.album_id=album_browse_order.album_id ORDER BY c.position LIMIT 1),'')
 WHERE album_id=OLD.album_id;
END;
CREATE TRIGGER IF NOT EXISTS album_order_artist_name AFTER UPDATE OF name ON artist BEGIN
 UPDATE album_browse_order SET artist_key=lower(NEW.name) WHERE artist_id=NEW.id;
END;
CREATE TRIGGER IF NOT EXISTS artwork_album_external_identity_insert AFTER INSERT ON album_external_identity BEGIN
 UPDATE album_artwork SET checked_at=0 WHERE album_id=NEW.album_id;
END;
CREATE TRIGGER IF NOT EXISTS artwork_album_external_identity_delete AFTER DELETE ON album_external_identity BEGIN
 UPDATE album_artwork SET checked_at=0 WHERE album_id=OLD.album_id;
END;
CREATE TRIGGER IF NOT EXISTS artwork_library_membership_insert AFTER INSERT ON library_membership BEGIN
 UPDATE album_artwork SET checked_at=0 WHERE album_id=(SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id=NEW.track_id);
END;
CREATE TRIGGER IF NOT EXISTS artwork_library_membership_delete AFTER DELETE ON library_membership BEGIN
 UPDATE album_artwork SET checked_at=0 WHERE album_id=(SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id=OLD.track_id);
END;
CREATE TRIGGER IF NOT EXISTS artwork_track_source_insert AFTER INSERT ON track_source BEGIN
 UPDATE album_artwork SET checked_at=0 WHERE album_id=(SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id=NEW.track_id);
END;
CREATE TRIGGER IF NOT EXISTS artwork_track_source_delete AFTER DELETE ON track_source BEGIN
 UPDATE album_artwork SET checked_at=0 WHERE album_id=(SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id=OLD.track_id);
END;
CREATE TRIGGER IF NOT EXISTS artwork_local_change AFTER UPDATE OF size_bytes,modified_ns,path,available ON local_file_observation
WHEN OLD.size_bytes<>NEW.size_bytes OR OLD.modified_ns<>NEW.modified_ns OR OLD.path<>NEW.path OR OLD.available<>NEW.available BEGIN
 UPDATE album_artwork SET checked_at=0 WHERE album_id IN (SELECT r.album_id FROM track_source ts JOIN track t ON t.id=ts.track_id JOIN release r ON r.id=t.release_id WHERE ts.source_id=NEW.source_id);
END;
CREATE TRIGGER IF NOT EXISTS album_order_effective_track_metadata_insert AFTER INSERT ON effective_track_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),printf('%012d',COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),999999999999)) FROM album_application_metadata a WHERE a.album_id=album_browse_order.album_id) WHERE album_id IN (SELECT r.album_id FROM release r JOIN track t ON t.release_id=r.id WHERE t.id=NEW.track_id);
END;
CREATE TRIGGER IF NOT EXISTS album_order_effective_track_metadata_update AFTER UPDATE OF year ON effective_track_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),printf('%012d',COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),999999999999)) FROM album_application_metadata a WHERE a.album_id=album_browse_order.album_id) WHERE album_id IN (SELECT r.album_id FROM release r JOIN track t ON t.release_id=r.id WHERE t.id=NEW.track_id);
END;
CREATE TRIGGER IF NOT EXISTS album_order_effective_track_metadata_delete AFTER DELETE ON effective_track_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),printf('%012d',COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),999999999999)) FROM album_application_metadata a WHERE a.album_id=album_browse_order.album_id) WHERE album_id IN (SELECT r.album_id FROM release r JOIN track t ON t.release_id=r.id WHERE t.id=OLD.track_id);
END;
CREATE TRIGGER IF NOT EXISTS album_order_release_application_metadata_insert AFTER INSERT ON release_application_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),printf('%012d',COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),999999999999)) FROM album_application_metadata a WHERE a.album_id=album_browse_order.album_id) WHERE album_id IN (SELECT album_id FROM release WHERE id=NEW.release_id);
END;
CREATE TRIGGER IF NOT EXISTS album_order_release_application_metadata_update AFTER UPDATE OF year ON release_application_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),printf('%012d',COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),999999999999)) FROM album_application_metadata a WHERE a.album_id=album_browse_order.album_id) WHERE album_id IN (SELECT album_id FROM release WHERE id=NEW.release_id);
END;
CREATE TRIGGER IF NOT EXISTS album_order_release_application_metadata_delete AFTER DELETE ON release_application_metadata BEGIN
 UPDATE album_browse_order SET (year,year_key)=(SELECT COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),printf('%012d',COALESCE(999999-COALESCE(a.year,(SELECT min(m.year) FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=a.album_id),(SELECT min(e.year) FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=a.album_id)),999999999999)) FROM album_application_metadata a WHERE a.album_id=album_browse_order.album_id) WHERE album_id IN (SELECT album_id FROM release WHERE id=OLD.release_id);
END;
PRAGMA user_version = 17;
COMMIT;
