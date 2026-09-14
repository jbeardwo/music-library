//! Opt-in local-folder probe. All library mutations are in memory; files are read only.
use music_library::{
    Library,
    catalog::CatalogProvider,
    domain::*,
    filesystem::LoftyMetadataExtractor,
    recording::{Outcome, Reply},
};
use music_library_musicbrainz::MusicBrainz;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 && !(args.len() == 3 && args[2] == "--programs") {
        return Err(
            "usage: recording_probe LOCAL_ALBUM_FOLDER CONFIRMED_RELEASE_GROUP_MBID [--programs]"
                .into(),
        );
    }
    if let Some(query) = args[1].strip_prefix("query:") {
        let page = MusicBrainz::new().search_albums(query, 0)?;
        println!("Diagnostic Album discovery: {page:#?}");
        if page.items.len() != 1 {
            return Err("supply a confirmed group ID from the diagnostic results".into());
        }
        args[1] = page.items[0].identity.external_id.clone();
    }
    let mut library = Library::open_in_memory()?;
    let root = library.register_local_root(&args[0])?;
    library.scan_local_root(&root, &mut LoftyMetadataExtractor)?;
    let files = library.list_discovery_candidates(None, 200)?;
    if files.is_empty() || files.len() == 200 {
        return Err("expected one bounded local Album folder".into());
    }
    let local_title = files[0].metadata.release_title.clone().unwrap_or_default();
    let credits = files[0]
        .metadata
        .release_artists
        .iter()
        .map(|name| ArtistCreditInput {
            name: name.clone(),
            role: None,
        })
        .collect();
    let imported = library.import_release(&ImportReleaseRequest {
        release_title: local_title,
        release_artists: credits,
        tracks: files
            .iter()
            .map(|f| ImportTrackInput {
                source_id: f.source_id.clone(),
                title_fallback: None,
                artists: vec![],
                disc_number: f.metadata.disc_number,
                track_number: f.metadata.track_number,
            })
            .collect(),
    })?;
    let album = library.album_for_release(&imported.release_id)?;
    library.attach_album_external_identity(
        &album.album_id,
        &ExternalIdentity {
            provider: "musicbrainz".into(),
            kind: "release_group".into(),
            external_id: args[1].clone(),
        },
    )?;
    let before = library.search(&SearchRequest {
        limit: 200,
        ..Default::default()
    })?;
    if args.len() == 3 {
        let identity = ExternalIdentity {
            provider: "musicbrainz".into(),
            kind: "release_group".into(),
            external_id: args[1].clone(),
        };
        let input = library
            .prepare_album_program(&album.album_id, &identity)?
            .ok_or("no local Tracks")?;
        let mut client = MusicBrainz::new();
        let start = std::time::Instant::now();
        let programs = client.album_programs(&identity)?;
        println!(
            "Programs retrieved in {:?}: {}",
            start.elapsed(),
            programs.note
        );
        for (index, program) in programs.programs.iter().enumerate() {
            println!(
                "Program {index} {:?}: fit {:?}",
                program.identity,
                music_library::album_program::program_fit(&input.tracks, program)
            );
            for local in &input.tracks {
                for (position, candidate) in program.tracks.iter().enumerate() {
                    println!(
                        "program={index} local={:?}/{:?} {:?} normalized={:?} duration={:?} recordings={:?}; provider={:?}/{:?} {:?} normalized={:?} duration={:?} recordings={:?} occurrences={:?}; {:?}",
                        local.evidence.disc,
                        local.evidence.number,
                        local.evidence.title,
                        music_library::album_program::comparison_title(
                            local.evidence.title.as_deref().unwrap_or("")
                        ),
                        local.evidence.duration_ms,
                        local.evidence.recording.identities,
                        candidate.disc,
                        candidate.number,
                        candidate.title,
                        music_library::album_program::comparison_title(
                            candidate.title.as_deref().unwrap_or("")
                        ),
                        candidate.duration_ms,
                        candidate.recording.identities,
                        candidate.identities,
                        music_library::album_program::inspect_candidate(local, candidate, position)
                    );
                }
            }
        }
        println!(
            "Selected program indices: {:?}",
            music_library::album_program::selected_programs(&input.tracks, &programs)
        );
        let start = std::time::Instant::now();
        let result = library.complete_album_program(music_library::album_program::Reply {
            input,
            result: Ok(programs),
        })?;
        println!(
            "Local compare/persistence {:?}: {result:#?}",
            start.elapsed()
        );
    } else {
        let input = library
            .prepare_recording_match(&album.album_id)?
            .ok_or("no eligible Tracks")?;
        println!(
            "Local Album: {} — {}; {} Tracks",
            album.title,
            album.artist_names,
            input.tracks.len()
        );
        let mut client = MusicBrainz::new();
        let page = client.recordings(&input.group)?;
        println!(
            "One logical Recording request: {} candidates; incomplete={}",
            page.items.len(),
            page.next_offset.is_some()
        );
        let outcome = library.complete_recording_match(Reply {
            input,
            result: Ok(page),
        })?;
        if let Outcome::Complete(rows) = outcome {
            for (local, result) in rows {
                println!("{}: {:?}", local.title, result);
            }
        }
    }
    assert_eq!(
        before,
        library.search(&SearchRequest {
            limit: 200,
            ..Default::default()
        })?
    );
    assert!(
        library
            .list_release_external_identities(&imported.release_id)?
            .is_empty()
    );
    for track in &imported.track_ids {
        assert!(library.list_track_external_identities(track)?.is_empty());
        assert!(library.available_playback_source(track)?.is_some());
    }
    println!(
        "Local metadata, availability and Track placements unchanged; no Release/Track provider identities inferred."
    );
    Ok(())
}
