//! Read-only bounded explicit Track searches in a disposable catalog-import database.
use music_library::{
    Library,
    domain::SearchRequest,
    song_resolution::{FeasibilityClass, Selection, SongSearch, assess, feasibility},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::PathBuf::from(
        std::env::var_os("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE")
            .ok_or("Set disposable /tmp database")?,
    );
    if !path.starts_with("/tmp") || !path.exists() {
        return Err("Requires existing disposable /tmp database".into());
    }
    let library = Library::open(&path)?;
    let db =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let rows = library.search(&SearchRequest {
        limit: 100,
        ..Default::default()
    })?;
    let mut spotify = music_library_spotify::Spotify::from_env()?;
    for title in ["El mañana", "Feel Good Inc.", "Dirty Harry", "DARE"] {
        let row = rows
            .iter()
            .find(|r| r.title.eq_ignore_ascii_case(title))
            .ok_or("Track absent")?;
        let input = library.song_resolution_input(&row.track_id)?;
        println!("INPUT {input:?}");
        let mut statement=db.prepare("SELECT c.position,a.id,a.name,COALESCE(c.credited_name,a.name),COALESCE(c.join_phrase,''),i.provider,i.kind,i.external_id FROM track_artist_credit c JOIN artist a ON a.id=c.artist_id LEFT JOIN artist_external_identity i ON i.artist_id=a.id WHERE c.track_id=?1 ORDER BY c.position,i.provider")?;
        for r in statement.query_map([row.track_id.as_ref()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        })? {
            println!("STRUCTURED CREDIT {:?}", r?);
        }
        let page = spotify.search_songs(&input)?;
        println!(
            "RESULT title={title:?} count={} more={} assessment={:?}",
            page.items.len(),
            page.next_offset.is_some(),
            assess(&input, &page)
        );
        let before = spotify.request_counts();
        let started = std::time::Instant::now();
        let classes: Vec<_> = page.items.iter().map(|c| feasibility(&input, c)).collect();
        let elapsed = started.elapsed();
        println!(
            "CLASSES preferred={} alternate={} hidden={} cost={elapsed:?}",
            classes
                .iter()
                .filter(|c| c.class == FeasibilityClass::Preferred)
                .count(),
            classes
                .iter()
                .filter(|c| c.class == FeasibilityClass::Alternate)
                .count(),
            classes
                .iter()
                .filter(|c| c.class == FeasibilityClass::Infeasible)
                .count()
        );
        for (candidate, class) in page.items.iter().zip(&classes) {
            println!("CANDIDATE {candidate:?} FEASIBILITY {class:?}");
        }
        let mut selection = Selection::new(input.clone(), page.items);
        println!("DEFAULT indices={:?}", selection.visible_indices());
        selection.show_all();
        assert_eq!(
            selection.visible_indices().len(),
            selection.candidates().len()
        );
        assert_eq!(spotify.request_counts(), before);
        println!(
            "SHOW ALL {} candidates, zero extra HTTP",
            selection.visible_indices().len()
        );
        assert_eq!(input, library.song_resolution_input(&row.track_id)?);
    }
    println!(
        "TOTAL token/catalog HTTP {:?}; display metadata unchanged; no acceptance",
        spotify.request_counts()
    );
    Ok(())
}
