//! Run on a disposable copy of the deterministic 200k library.
use music_library::Library;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).expect("disposable database path");
    let mut l = Library::open(&path)?;
    let id = l.create_playlist("Performance fixture")?;
    let db = rusqlite::Connection::open(&path)?;
    db.execute("INSERT INTO playlist_entry(id,playlist_id,track_id,position) SELECT ?1||'-entry-'||id,?1,id,row_number() OVER(ORDER BY id)-1 FROM track",[&id])?;
    for _ in 0..3 {
        let start = std::time::Instant::now();
        let first = l.playlist_entries(&id, None, 201)?;
        let deep = l.playlist_entries(&id, Some(190_000), 201)?;
        assert!(first.len() <= 201 && deep.len() <= 201);
        assert_eq!(first[0].playlist_position, Some(1));
        assert_eq!(deep[0].playlist_position, Some(190_002));
        assert_eq!(deep.last().unwrap().playlist_position, Some(190_202));
        println!("first and deep page: {:?}", start.elapsed());
        let start = std::time::Instant::now();
        let back =
            l.selected_playlist_entries_before(std::slice::from_ref(&id), &deep[0].cursor, 201)?;
        assert!(
            back.windows(2)
                .all(|w| w[0].cursor.position > w[1].cursor.position)
        );
        let head =
            l.selected_playlist_entries_before(std::slice::from_ref(&id), &first[200].cursor, 201)?;
        assert_eq!(
            head.iter().rev().map(|r| &r.id).collect::<Vec<_>>(),
            first[..200].iter().map(|r| &r.id).collect::<Vec<_>>()
        );
        println!("deep and near-head reverse chunks: {:?}", start.elapsed());
    }
    for _ in 0..3 {
        let start = std::time::Instant::now();
        let details = l.playlist_details(&id)?.unwrap();
        assert_eq!(details.entry_count, 200_000);
        assert_eq!(details.unknown_duration_count, 200_000);
        assert_eq!(details.known_duration_ms, 0);
        println!("200k entry aggregate: {:?}", start.elapsed());
    }
    for column in [
        music_library::playlist::Column::Title,
        music_library::playlist::Column::Artist,
        music_library::playlist::Column::Album,
        music_library::playlist::Column::Length,
    ] {
        let sort = music_library::playlist::ViewSort {
            column,
            descending: false,
        };
        let start = std::time::Instant::now();
        let first = l.playlist_view(std::slice::from_ref(&id), sort, None, 201, false)?;
        println!(
            "{column:?} 200k projection and first page: {:?}",
            start.elapsed()
        );
        let start = std::time::Instant::now();
        let second = l.playlist_view(
            std::slice::from_ref(&id),
            sort,
            Some(&first[200].cursor),
            201,
            false,
        )?;
        assert_eq!(first.len(), 201);
        assert_eq!(second.len(), 201);
        println!("{column:?} indexed next sorted page: {:?}", start.elapsed());
        let key = match column {
            music_library::playlist::Column::Title => "e.title COLLATE NOCASE",
            music_library::playlist::Column::Artist => "e.artist_names COLLATE NOCASE",
            music_library::playlist::Column::Album => "a.title COLLATE NOCASE",
            music_library::playlist::Column::Length => "COALESCE(e.duration_ms,-1)",
            _ => unreachable!(),
        };
        // OFFSET is confined to this diagnostic's reference cursor construction;
        // the product uses the indexed keyset query for the actual deep page.
        let sql = format!(
            "SELECT p.id,p.position,{key} FROM playlist_entry p JOIN effective_track_metadata e ON e.track_id=p.track_id JOIN track t ON t.id=p.track_id JOIN release r ON r.id=t.release_id JOIN album_application_metadata a ON a.album_id=r.album_id WHERE p.playlist_id=?1 ORDER BY {key},p.position,p.id LIMIT 1 OFFSET 190000"
        );
        let cursor = db.query_row(&sql, [&id], |r| {
            let mut c = music_library::browse::Cursor {
                id: r.get(0)?,
                position: r.get(1)?,
                release: id.clone(),
                ..Default::default()
            };
            if column == music_library::playlist::Column::Length {
                c.disc = r.get(2)?;
            } else {
                c.title = r.get(2)?;
            }
            Ok(c)
        })?;
        for descending in [false, true] {
            let start = std::time::Instant::now();
            let deep = l.playlist_view(
                std::slice::from_ref(&id),
                music_library::playlist::ViewSort { column, descending },
                Some(&cursor),
                201,
                false,
            )?;
            assert_eq!(deep.len(), 201);
            assert!(
                deep.iter()
                    .all(|r| r.playlist_position == Some(r.cursor.position as u64 + 1))
            );
            println!(
                "{column:?} deep keyset page descending={descending}: {:?}",
                start.elapsed()
            );
        }
    }
    let mut plan = db.prepare("EXPLAIN QUERY PLAN SELECT pl.id,COUNT(p.id),SUM(e.duration_ms) FROM playlist pl LEFT JOIN playlist_entry p INDEXED BY playlist_entry_order ON p.playlist_id=pl.id LEFT JOIN effective_track_metadata e ON e.track_id=p.track_id WHERE pl.id=?1 GROUP BY pl.id")?;
    let plans = plan
        .query_map([&id], |r| r.get::<_, String>(3))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    assert!(
        plans.iter().any(|p| p.contains("playlist_entry_order")),
        "{plans:?}"
    );
    assert!(
        !plans
            .iter()
            .any(|p| p.starts_with("SCAN p") || p.starts_with("SCAN e")),
        "{plans:?}"
    );
    println!("aggregate: {plans:?}");
    let mut plan = db.prepare("EXPLAIN QUERY PLAN SELECT COUNT(*) FROM playlist_entry WHERE playlist_id=?1 AND position<=?2")?;
    let plans = plan
        .query_map(rusqlite::params![id, 190_001], |r| r.get::<_, String>(3))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    assert!(
        plans.iter().any(|p| p.contains("playlist_entry_order")),
        "{plans:?}"
    );
    println!("page ordinal: {plans:?}");
    let start = std::time::Instant::now();
    let (queue, _) = l.library_queue_reader()?.read_playlist(&id, None)?;
    println!(
        "{} entry queue snapshot: {:?}",
        queue.len(),
        start.elapsed()
    );
    let mut q=db.prepare("EXPLAIN QUERY PLAN SELECT p.track_id FROM playlist_entry p WHERE p.playlist_id=?1 AND p.position>?2 ORDER BY p.position LIMIT 201")?;
    for row in q.query_map(rusqlite::params![id, 190_000], |r| r.get::<_, String>(3))? {
        println!("{}", row?);
    }
    let mut q=db.prepare("EXPLAIN QUERY PLAN SELECT p.id FROM playlist_entry p WHERE p.playlist_id IN (SELECT value FROM json_each(?1)) AND (p.playlist_id,p.position,p.id)<(?2,?3,?4) AND p.position<=?3 ORDER BY p.playlist_id DESC,p.position DESC,p.id DESC LIMIT 201")?;
    let plans = q
        .query_map(
            rusqlite::params![serde_json::to_string(&[&id])?, id, 190_000, "entry"],
            |r| r.get::<_, String>(3),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    assert!(!plans.iter().any(|p| p.contains("TEMP B-TREE")));
    for plan in plans {
        println!("reverse entries: {plan}");
    }
    l.delete_playlist(&id)?;
    Ok(())
}
