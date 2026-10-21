//! Offline shared-correspondence audit on an explicitly disposable profile copy.
use music_library::{
    Library,
    browse::{Pane, Request},
    domain::AlbumId,
};
use serde_json::json;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("disposable database required")?;
    if !path.starts_with("/tmp/mb-components-") {
        return Err("disposable copy required".into());
    }
    let lib = Library::open(&path)?;
    let mut after = None;
    let mut albums = vec![];
    loop {
        let rows = lib.browse(&Request {
            pane: Pane::Albums,
            limit: 200,
            after: after.clone(),
            ..Default::default()
        })?;
        for row in &rows {
            if ![
                "Angle",
                "Dreams",
                "Work",
                "Underwater",
                "Freshman Year",
                "100 Onces",
                "More Skin With Milk-Mouth (2015 Remaster)",
            ]
            .contains(&row.title.as_str())
            {
                continue;
            }
            let id = AlbumId(row.id.clone());
            let local = lib.local_album_tracks(&id)?;
            let review = lib.musicbrainz_review(&id)?;
            let candidates=review.candidates.iter().filter_map(|c|c.programs.as_ref().or(c.inspection.as_ref()).map(|p|json!({"metadata":c.metadata,"raw_programs":p,"assessment":music_library::musicbrainz_recording::assess(&local,p,&row.title),"projection":lib.musicbrainz_program_review(&id,p).unwrap()}))).collect::<Vec<_>>();
            albums.push(json!({"id":id,"title":row.title,"local":local.iter().map(|t|json!({"id":t.track_id,"evidence":t.evidence,"components":music_library::matching::alternate_title_components(t.evidence.title.as_deref().unwrap_or(""))})).collect::<Vec<_>>(),"candidates":candidates,"input":format!("{:?}",lib.prepare_musicbrainz_discovery(&id)?)}));
        }
        after = rows.last().map(|r| r.cursor.clone());
        if rows.len() < 200 {
            break;
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"coverage":lib.connection_coverage("musicbrainz")?,"albums":albums})
        )?
    );
    Ok(())
}
