pub fn downgrade(db: &rusqlite::Connection) {
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
