DROP TRIGGER playlist_entry_insert_revision;
DROP TRIGGER playlist_entry_delete_revision;
DROP TRIGGER playlist_entry_update_revision;
ALTER TABLE playlist DROP COLUMN content_revision;
