BEGIN;
ALTER TABLE playlist ADD COLUMN content_revision INTEGER NOT NULL DEFAULT 0;
CREATE TRIGGER playlist_entry_insert_revision AFTER INSERT ON playlist_entry BEGIN
    UPDATE playlist SET content_revision=content_revision+1 WHERE id=NEW.playlist_id;
END;
CREATE TRIGGER playlist_entry_delete_revision AFTER DELETE ON playlist_entry BEGIN
    UPDATE playlist SET content_revision=content_revision+1 WHERE id=OLD.playlist_id;
END;
CREATE TRIGGER playlist_entry_update_revision AFTER UPDATE OF playlist_id,track_id,position ON playlist_entry
WHEN OLD.playlist_id!=NEW.playlist_id OR OLD.track_id!=NEW.track_id OR OLD.position!=NEW.position BEGIN
    UPDATE playlist SET content_revision=content_revision+1 WHERE id IN (OLD.playlist_id,NEW.playlist_id);
END;
PRAGMA user_version=25;
COMMIT;
