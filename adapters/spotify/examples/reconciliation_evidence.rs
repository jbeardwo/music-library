//! Bounded, read-only evidence capture for selected local Albums. No associations are changed.
use music_library::{
    album_candidates,
    album_matching::{Preparation, accepted_album_confirmed, resolve_artist},
    album_program,
    catalog::CatalogProvider,
    domain::AlbumId,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let database = args
        .next()
        .ok_or("usage: reconciliation_evidence DATABASE ALBUM_ID...")?;
    let mut library = music_library::Library::open(database)?;
    let mut albums = args.collect::<Vec<_>>();
    let apply = albums.iter().any(|arg| arg == "--apply");
    albums.retain(|arg| arg != "--apply");
    let mut provider = music_library_spotify::Spotify::from_env()?;
    for album in albums {
        let album = AlbumId(album);
        let Preparation::Ready(input) =
            library.prepare_album_match_for(&album, &music_library_spotify::matching_scope())?
        else {
            println!("Already established or not eligible: {}", album.as_ref());
            continue;
        };
        let (artist, mut page, established) = match &input.known_artist {
            Some(id) => (id.clone(), provider.artist_albums(id, &input.title)?, true),
            None => match resolve_artist(&input.artist, &provider.search_artists(&input.artist)?) {
                Ok(id) => {
                    let page = provider.artist_albums(&id, &input.title)?;
                    (id, page, true)
                }
                Err(music_library::album_matching::MatchOutcome::ArtistAmbiguous(candidates)) => {
                    let page = provider.albums_for_artists(
                        &candidates
                            .iter()
                            .map(|c| c.identity.clone())
                            .collect::<Vec<_>>(),
                        &input.title,
                    )?;
                    let pair = music_library::album_matching::corroborate_artist_album(
                        &input.artist,
                        &input.title,
                        &candidates,
                        &page,
                    );
                    println!("JOINT ARTIST/ALBUM {pair:?}");
                    let established = pair.0.is_some();
                    let identity = pair.0.or_else(|| {
                        page.items
                            .first()
                            .and_then(|c| c.artist_ids.first())
                            .cloned()
                    });
                    let Some(id) = identity else {
                        println!("Artist unresolved; withholding; no bounded Album candidates");
                        continue;
                    };
                    (id, page, established)
                }
                Err(reason) => {
                    println!("Artist unresolved: {reason:?}");
                    continue;
                }
            },
        };
        let local = library.local_album_tracks(&album)?;
        if page.items.is_empty() {
            let control = provider.search_albums(
                &format!("album:\"{}\" artist:\"{}\"", input.title, input.artist),
                0,
            )?;
            println!("RAW SEARCH CONTROL {control:?}");
            page.items = control
                .items
                .into_iter()
                .map(|c| music_library::catalog::ArtistAlbumCandidate {
                    identity: c.identity,
                    title: c.title,
                    artist: c.artist,
                    artist_ids: c.credits.into_iter().filter_map(|c| c.identity).collect(),
                    primary_type: c.primary_type,
                    date: c.date,
                    comment: c.comment,
                })
                .collect();
            page.next_offset = control.next_offset;
        }
        let counts: Vec<_> = page
            .items
            .iter()
            .map(|c| provider.album_candidate_track_count(&c.identity))
            .collect();
        println!(
            "LOCAL {:?} / {:?}; known_artist={:?}; resolved={artist:?}; count={}; required={}; candidates={}; more={:?}",
            input.artist,
            input.title,
            input.known_artist,
            local.len(),
            album_candidates::required_tracks(&local),
            page.items.len(),
            page.next_offset
        );
        println!(
            "TITLE DECISION {:?}",
            accepted_album_confirmed(&input.title, &artist, &page, false)
        );
        let mut cache = vec![];
        let resolution = if established {
            album_candidates::resolve_canonical(
                &mut provider,
                (&input.raw_title, input.date),
                &artist,
                &page,
                false,
                &local,
                &mut cache,
                &input.equivalent_artists,
                &input.evidence,
            )?
        } else {
            let mut report = album_candidates::initial_report(
                &provider,
                (&input.raw_title, input.date),
                None,
                &page,
                &local,
            );
            report.reasons = vec![album_candidates::Reason::ArtistUnresolved];
            album_candidates::Resolution {
                outcome: music_library::album_matching::MatchOutcome::NoConfidentMatch,
                report,
            }
        };
        println!("AUTOMATIC DECISION {:?}", resolution.outcome);
        println!(
            "STRUCTURED REPORT {}",
            serde_json::to_string(&resolution.report)?
        );
        println!(
            "{}",
            resolution
                .report
                .explain_track(local.first().map_or("", |t| t.track_id.as_ref()))
        );
        let mut captured = vec![];
        let mut candidates = page.items.clone();
        candidates.sort_by(|a, b| a.identity.external_id.cmp(&b.identity.external_id));
        for c in candidates
            .iter()
            .take(album_candidates::MAX_PROGRAM_CANDIDATES)
        {
            let programs = provider.album_programs(&c.identity)?;
            println!(
                "CANDIDATE {:?}; fit={:?}",
                c,
                album_candidates::fit(&local, &programs)
            );
            for p in &programs.programs {
                println!(
                    "PROGRAM complete={} count={} fit={:?}",
                    p.complete,
                    p.tracks.len(),
                    album_program::program_fit(&local, p)
                );
                for t in &local {
                    let comparisons:Vec<_> = p.tracks.iter().enumerate().filter(|(i,c)| t.evidence.number==c.number || t.evidence.number==Some(*i as u32+1))
                        .map(|(i,c)| serde_json::json!({"provider":c,"check":format!("{:?}",album_program::inspect_candidate(t,c,i))})).collect();
                    println!("TRACK {:?}: {comparisons:?}", t.evidence.title);
                }
            }
            captured.push(serde_json::json!({"identity":c.identity,"title":c.title,"artist":c.artist,"artist_ids":c.artist_ids,"primary_type":c.primary_type,"date":c.date,"count":counts[page.items.iter().position(|v|v.identity==c.identity).unwrap()],"programs":programs.programs.iter().map(|p|serde_json::json!({"complete":p.complete,"tracks":p.tracks})).collect::<Vec<_>>()}));
        }
        if apply && established {
            use music_library::album_matching::{MatchOutcome, MatchReply};
            let presentation = match &resolution.outcome {
                MatchOutcome::Matched(id) | MatchOutcome::MatchedClose(id) => {
                    page.items.iter().find(|c| &c.identity == id).cloned()
                }
                _ => None,
            };
            let selected = presentation.as_ref().map(|c| c.identity.clone());
            let outcome = library.complete_album_match_for(
                MatchReply {
                    input: input.clone(),
                    artist: Some(artist.clone()),
                    outcome: resolution.outcome,
                    matched_album: presentation,
                    diagnostic: Some(resolution.report),
                },
                &music_library_spotify::matching_scope(),
            )?;
            if let Some(id) = selected
                && matches!(
                    outcome,
                    MatchOutcome::Matched(_) | MatchOutcome::MatchedClose(_)
                )
                && let Some(input) = library.prepare_album_program(&album, &id)?
            {
                let programs = provider.album_programs(&id)?;
                library.complete_album_program(album_program::Reply {
                    input,
                    result: Ok(programs),
                })?;
            }
            println!(
                "PERSISTED {:?}: {} associated Tracks",
                outcome,
                library
                    .provider_track_associations(&album, "spotify")?
                    .iter()
                    .filter(|(_, m)| !m.occurrences.is_empty())
                    .count()
            );
        }
        println!(
            "CAPTURE {}",
            serde_json::json!({"album":album.as_ref(),"title":input.title,"artist":artist,"date":input.date.map(|d|format!("{d:?}")),"local":local.iter().map(|t|serde_json::json!({"track_id":t.track_id.as_ref(),"recording_id":t.recording_id.as_ref(),"evidence":t.evidence})).collect::<Vec<_>>(),"candidates":captured,"next_offset":page.next_offset})
        );
    }
    Ok(())
}
