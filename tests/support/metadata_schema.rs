pub fn downgrade(db: &rusqlite::Connection) {
    db.execute_batch("DROP INDEX IF EXISTS connections_album_order; DROP VIEW IF EXISTS connected_local_track; DROP VIEW IF EXISTS connected_musicbrainz_track; DROP TRIGGER IF EXISTS connections_local_attached; DROP TRIGGER IF EXISTS connections_local_available; DROP TRIGGER IF EXISTS connections_recording_attached;").unwrap();
    // Profile preferences postdate these historical schema fixtures.
    db.execute_batch("DROP TABLE IF EXISTS hidden_artist_preference; DROP TABLE IF EXISTS ignored_track_preference;").unwrap();
    if db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='album_metadata_override')",
            [],
            |r| r.get::<_, bool>(0),
        )
        .unwrap()
    {
        db.execute_batch(include_str!("drop_metadata_editor.sql"))
            .unwrap();
    }
}
