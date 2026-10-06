//! Explicit bounded catalog import into a disposable validation database.
use music_library::{Library, catalog::CatalogProvider, domain::ExternalIdentity};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let db = args.next().ok_or("DATABASE GROUP_MBID")?;
    let group = args.next().ok_or("GROUP_MBID")?;
    let mut provider = music_library_musicbrainz::MusicBrainz::new();
    let page = provider.releases(
        &ExternalIdentity {
            provider: "musicbrainz".into(),
            kind: "release_group".into(),
            external_id: group,
        },
        0,
    )?;
    let candidate = page.items.first().ok_or("No release on bounded page")?;
    let release = provider.release(&candidate.identity)?;
    let mut l = Library::open(db)?;
    let imported = l.add_catalog_release(&release)?;
    let input = l.song_resolution_input(&imported.track_ids[0])?;
    println!(
        "{}",
        serde_json::json!({"album_title":release.album.title,"release_title":release.title,"date":release.album.date,"release_type":release.album.release_type,"evidence":input.evidence,"track_ids":imported.track_ids.iter().map(|id|id.as_ref()).collect::<Vec<_>>(),"album_id":l.album_for_release(&imported.release_id)?.album_id.as_ref()})
    );
    Ok(())
}
