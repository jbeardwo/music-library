//! Local-only timings; optional closed 200k fixture copied into a disposable database.
use music_library::{
    Library, Result,
    album_matching::{AlbumMatcher, AutoMatchPolicy, MatchOutcome},
    catalog::*,
    domain::*,
    filesystem::MetadataExtractor,
};
use std::{
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};
struct Tags(String);
impl MetadataExtractor for Tags {
    fn supports(&self, _: &Path) -> bool {
        true
    }
    fn read(&mut self, path: &Path) -> Result<ObservedMetadata> {
        Ok(ObservedMetadata {
            track_title: Some(format!(
                "Song {}",
                path.file_stem().unwrap().to_string_lossy()
            )),
            release_title: Some(self.0.clone()),
            track_artists: vec!["Timing Artist".into()],
            disc_number: Some(1),
            track_number: Some(1),
            ..Default::default()
        })
    }
}
struct Provider;
impl CatalogProvider for Provider {
    fn search_artists(
        &mut self,
        name: &str,
    ) -> std::result::Result<Page<ArtistCandidate>, CatalogError> {
        Ok(Page {
            next_offset: None,
            items: vec![ArtistCandidate {
                aliases: vec![],
                identity: ExternalIdentity {
                    provider: "musicbrainz".into(),
                    kind: "artist".into(),
                    external_id: "00000000-0000-4000-8000-000000000002".into(),
                },
                name: name.into(),
                comment: String::new(),
                country: String::new(),
                artist_type: String::new(),
                score: None,
            }],
        })
    }
    fn artist_albums(
        &mut self,
        artist: &ExternalIdentity,
        title: &str,
    ) -> std::result::Result<Page<ArtistAlbumCandidate>, CatalogError> {
        Ok(Page {
            next_offset: None,
            items: vec![ArtistAlbumCandidate {
                artist: "Artist".into(),
                primary_type: String::new(),
                title: title.into(),
                artist_ids: vec![artist.clone()],
                identity: ExternalIdentity {
                    provider: "musicbrainz".into(),
                    kind: "release_group".into(),
                    external_id: "00000000-0000-4000-8000-000000000001".into(),
                },

                date: String::new(),

                comment: String::new(),
            }],
        })
    }
    fn search_albums(
        &mut self,
        _: &str,
        _: u32,
    ) -> std::result::Result<Page<AlbumCandidate>, CatalogError> {
        unreachable!()
    }
    fn releases(
        &mut self,
        _: &ExternalIdentity,
        _: u32,
    ) -> std::result::Result<Page<ReleaseCandidate>, CatalogError> {
        unreachable!()
    }
    fn release(&mut self, _: &ExternalIdentity) -> std::result::Result<Release, CatalogError> {
        unreachable!()
    }
}
fn report(name: &str, mut samples: Vec<Duration>) {
    samples.sort();
    println!(
        "{name}: median {:.3} µs, p95 {:.3} µs ({} samples)",
        samples[samples.len() / 2].as_secs_f64() * 1e6,
        samples[(samples.len() * 95).div_ceil(100) - 1].as_secs_f64() * 1e6,
        samples.len()
    );
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let db = temp.path().join("db");
    if let Some(seed) = std::env::args_os().nth(1) {
        std::fs::copy(seed, &db)?;
    }
    let mut library = Library::open(db)?;
    let folder = temp.path().join("music");
    std::fs::create_dir(&folder)?;
    let root = library.register_local_root(&folder)?;
    let (send, recv) = mpsc::channel();
    let mut matcher = AlbumMatcher::new(Provider, move |reply| send.send(reply).unwrap(), |_| {})?;
    for count in [1, 3, 15] {
        let mut imports = vec![];
        let mut eligibility = vec![];
        let mut dispatch = vec![];
        let mut attach = vec![];
        for trial in 0..21 {
            for n in 0..count {
                std::fs::write(folder.join(format!("{count}-{trial}-{n}.mp3")), b"fixture")?;
            }
            library.scan_local_root(&root, &mut Tags(format!("Timing Album {count}-{trial}")))?;
            let request = ImportReleaseRequest {
                release_title: format!("Timing Album {count}-{trial}"),
                release_artists: vec![],
                tracks: library
                    .list_discovery_candidates(None, 200)?
                    .into_iter()
                    .map(|c| ImportTrackInput {
                        source_id: c.source_id,
                        title_fallback: None,
                        artists: vec![],
                        disc_number: Some(1),
                        track_number: Some(1),
                    })
                    .collect(),
            };
            let start = Instant::now();
            let imported = library.import_release(&request)?;
            let import_time = start.elapsed();
            let id = library.album_for_release(&imported.release_id)?.album_id;
            let start = Instant::now();
            std::hint::black_box(library.prepare_album_match(&id)?);
            let eligibility_time = start.elapsed();
            let start = Instant::now();
            matcher.after_import(&library, &[imported], AutoMatchPolicy::default())?;
            let dispatch_time = start.elapsed();
            let reply = recv.recv_timeout(Duration::from_secs(5))?;
            let start = Instant::now();
            assert!(matches!(
                matcher.complete(&mut library, reply),
                MatchOutcome::Matched(_)
            ));
            let attach_time = start.elapsed();
            if trial > 0 {
                imports.push(import_time);
                eligibility.push(eligibility_time);
                dispatch.push(dispatch_time);
                attach.push(attach_time);
            }
        }
        report(&format!("{count} Tracks import/commit"), imports);
        report("eligibility", eligibility);
        report("post-import dispatch (includes eligibility)", dispatch);
        report("identity attachment/revalidation/commit", attach);
    }
    Ok(())
}
