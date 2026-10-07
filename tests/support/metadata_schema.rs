pub fn downgrade(db: &rusqlite::Connection) {
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
