//! Run on a disposable copy of the deterministic 200k library.
use music_library::Library;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).expect("disposable database path");
    let mut l = Library::open(&path)?;
    let id = l.create_playlist("Performance fixture")?;
    let db = rusqlite::Connection::open(&path)?;
    db.execute("INSERT INTO playlist_entry(id,playlist_id,track_id,position) SELECT 'playlist-'||id,?1,id,row_number() OVER(ORDER BY id)-1 FROM track",[&id])?;
    for _ in 0..3 {
        let start = std::time::Instant::now();
        let first = l.playlist_entries(&id, None, 201)?;
        let deep = l.playlist_entries(&id, Some(190_000), 201)?;
        assert!(first.len() <= 201 && deep.len() <= 201);
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
