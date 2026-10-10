//! Cached evidence audit; pass a disposable database. No provider requests.
use music_library::{
    Library, album_matching::Preparation, catalog::Page, song_resolution::evaluate_attempt,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("DISPOSABLE_DATABASE")?;
    let mut library = Library::open(&path)?;
    let market = library
        .retained_spotify_market()?
        .ok_or("No single retained market")?;
    for group in library.spotify_review_groups(None, 200)? {
        let Preparation::Ready(input) = library.spotify_album_review_input(&group.id)? else {
            continue;
        };
        let Some((report, _)) =
            music_library_spotify::Spotify::cached_album_review(&path, &market, &input)?
        else {
            println!("{}: no retained discovery", group.title);
            continue;
        };
        println!("{}: {}", group.title, report.decision);
        for (rank, c) in report.candidates.iter().enumerate() {
            println!(
                "  rank {} {} {:?}; warnings {:?}; {} anchors",
                rank + 1,
                c.title.provider_raw,
                c.reasons,
                c.warnings,
                c.programs.iter().map(|p| p.anchors).sum::<usize>()
            );
        }
        for track in library.spotify_retry_tracks(std::slice::from_ref(&group.id), 20)? {
            if let Some((selection, more)) =
                music_library_spotify::album_context::selection(&library, &track, &market)?
            {
                let current = library.song_resolution_input(&track)?;
                let page = Page {
                    items: selection.candidates().to_vec(),
                    next_offset: more.then_some(10),
                };
                for index in selection.visible_indices() {
                    if let Some(e) =
                        evaluate_attempt(selection.input(), &current, &page, index, &[])
                    {
                        println!(
                            "  {} -> {} [{}] {:?}: {:?}",
                            current.title,
                            page.items[index].title,
                            selection.candidate_provenance(index),
                            e.final_decision,
                            e.primary_blocker
                        );
                    }
                }
            }
        }
        let batch = music_library_spotify::album_context::reconcile(
            &mut library,
            std::slice::from_ref(&group.id),
            &market,
        )?;
        println!(
            "  local completion: {} accepted, {} remain, errors {:?}",
            batch.accepted.len(),
            batch.still_unresolved,
            batch.errors
        );
    }
    println!("Provider requests: 0 (cache-only methods)");
    Ok(())
}
