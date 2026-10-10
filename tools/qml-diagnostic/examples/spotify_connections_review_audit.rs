//! Offline audit of a disposable real Library snapshot; never requests Spotify.
use music_library::{
    Library,
    album_matching::Preparation,
    browse::{Pane, Request},
    song_resolution::{Assessment, assess},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("DISPOSABLE_DATABASE")?;
    let mut library = Library::open(&path)?;
    let groups = library.spotify_review_groups(None, 200)?;
    println!("{} unresolved Album groups", groups.len());
    for group in &groups {
        if group.status == "Needs retry" {
            println!(
                "Retry context: {} · {} · eligible={} · {} Tracks",
                group.title,
                group.status,
                library.spotify_album_needs_retry(&group.id)?,
                group.unresolved
            );
        }
    }
    let rows = library.browse(&Request {
        pane: Pane::Songs,
        unresolved_spotify: true,
        limit: 201,
        ..Default::default()
    })?;
    let mut multi = 0;
    let mut winners = 0;
    for row in rows {
        let track = music_library::domain::TrackId(row.id);
        if let Some((input, mut page)) = library.cached_spotify_track_attempt(&track)? {
            if page.items.len() < 2 {
                continue;
            }
            multi += 1;
            let first = assess(&input, &page);
            let selected = match first {
                Assessment::Unique(n) => Some(page.items[n].identity.clone()),
                _ => None,
            };
            page.items.reverse();
            let reversed = match assess(&input, &page) {
                Assessment::Unique(n) => Some(page.items[n].identity.clone()),
                _ => None,
            };
            assert_eq!(selected, reversed);
            if let Some(id) = selected {
                winners += 1;
                println!(
                    "Order-independent Track winner: {} — {} · {}",
                    input.artist, input.title, id.external_id
                );
            }
        }
    }
    let mut album_candidates = 0;
    let mut plans = 0;
    for group in &groups {
        if let Preparation::Ready(input) = library.spotify_album_review_input(&group.id)?
            && let Some((report, programs)) =
                music_library_spotify::Spotify::cached_album_review(&path, "US", &input)?
        {
            album_candidates += report.candidates.len();
            println!(
                "Album inspection: {} · {} cached candidates",
                group.title,
                report.candidates.len()
            );
            for (rank, candidate) in report.candidates.iter().enumerate() {
                println!(
                    "  Our rank {}; provider rank {:?}; warnings {:?}",
                    rank + 1,
                    candidate.provider_rank,
                    candidate.warnings
                );
                for p in &candidate.programs {
                    println!(
                        "  Alignment {} anchors, strong={}, unreliable positions={}: {}",
                        p.anchors,
                        p.alignment.strong,
                        p.alignment.unreliable_positions,
                        p.tracks
                            .iter()
                            .map(|t| format!(
                                "{}.{} {} -> {}.{} {} [{:?}]",
                                t.local.disc.unwrap_or(1),
                                t.local.number.unwrap_or(0),
                                t.local.title.as_deref().unwrap_or(""),
                                t.provider.as_ref().and_then(|p| p.disc).unwrap_or(0),
                                t.provider.as_ref().and_then(|p| p.number).unwrap_or(0),
                                t.provider
                                    .as_ref()
                                    .and_then(|p| p.title.as_deref())
                                    .unwrap_or("—"),
                                t.decision
                            ))
                            .collect::<Vec<_>>()
                            .join("; ")
                    );
                }
                println!(
                    "  {} — {} · {} · {}",
                    candidate.title.provider_raw,
                    candidate.artist,
                    candidate.decision,
                    candidate
                        .reasons
                        .iter()
                        .map(|r| r.label())
                        .collect::<Vec<_>>()
                        .join("; ")
                );
            }
            if std::env::args().nth(2).as_deref() == Some(group.title.as_str()) {
                let plans_for_album = report
                    .candidates
                    .iter()
                    .filter(|c| c.artist_accepted)
                    .filter_map(|c| programs.iter().find(|p| p.album == c.identity))
                    .filter_map(|p| {
                        library
                            .prepare_spotify_album_confirmation(&group.id, p)
                            .ok()
                    })
                    .collect::<Vec<_>>();
                if let Some(plan) = plans_for_album.iter().min_by_key(|p| {
                    p.matches()
                        .iter()
                        .filter(|m| !m.warnings.is_empty())
                        .count()
                }) {
                    let warnings = plan
                        .matches()
                        .iter()
                        .filter(|m| !m.warnings.is_empty())
                        .count();
                    println!(
                        "Explicit disposable Album confirmation: {} · {} positioned matches · {} significant differences · {} unmatched canonical Tracks · {} unmatched provider Tracks",
                        group.title,
                        plan.matches().len(),
                        warnings,
                        plan.unmatched_local.len(),
                        plan.unmatched_provider
                    );
                    let n = library.confirm_spotify_album(plan, true)?;
                    println!(
                        "Explicit confirmation connected {n}; remaining Album unresolved={}",
                        library
                            .spotify_review_groups(None, 200)?
                            .iter()
                            .find(|g| g.id == group.id)
                            .map(|g| g.unresolved)
                            .unwrap_or(0)
                    );
                    plans += 1;
                }
                continue;
            }
            // Only confirm a naturally accepted candidate on this disposable copy.
            if let Some(candidate) = report.candidates.iter().find(|c| c.decision == "Accepted")
                && let Some(p) = programs.iter().find(|p| p.album == candidate.identity)
            {
                let plan = library.prepare_spotify_album_confirmation(&group.id, p)?;
                if !plan.requires_confirmation() && !plan.matches().is_empty() {
                    let n = library.confirm_spotify_album(&plan, false)?;
                    plans += 1;
                    println!("Confirmed Album {}: {} positioned Tracks", group.title, n);
                }
            }
        }
    }
    println!(
        "Multi-candidate Track pages={multi}; order-independent winners={winners}; cached Album candidates={album_candidates}; safe real Album confirmations={plans}; provider requests=0"
    );
    Ok(())
}
