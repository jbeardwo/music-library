//! Cache-only validation of both real datasets. No MusicBrainz adapter/network dependency.
use music_library::{
    Library,
    album_matching::{MatchOutcome, MatchReply, Preparation},
    album_program::Programs,
    catalog::*,
    domain::*,
    program_correspondence as pc,
};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
struct Retained(Programs);
impl CatalogProvider for Retained {
    fn search_albums(&mut self, _: &str, _: u32) -> Result<Page<AlbumCandidate>, CatalogError> {
        panic!("validation must be offline")
    }
    fn releases(
        &mut self,
        _: &ExternalIdentity,
        _: u32,
    ) -> Result<Page<ReleaseCandidate>, CatalogError> {
        panic!("validation must be offline")
    }
    fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
        panic!("validation must be offline")
    }
    fn album_programs(&mut self, _: &ExternalIdentity) -> Result<Programs, CatalogError> {
        Ok(self.0.clone())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let evidence: Value = serde_json::from_str(include_str!(
        "../docs/musicbrainz-matching-audit-evidence.json"
    ))?;
    let prefix = std::env::args()
        .nth(1)
        .ok_or("NEW /tmp/mb-phase12-NAME prefix required")?;
    if !prefix.starts_with("/tmp/mb-phase12-") {
        return Err("disposable prefix required".into());
    }
    let mut reports = vec![];
    for (name, source) in [
        ("fresh", "/tmp/music-library-fresh.sqlite"),
        ("profile", "/tmp/music-library-ui-test.sqlite"),
    ] {
        let path = format!("{prefix}-{name}.sqlite");
        if std::path::Path::new(&path).exists() {
            return Err("refusing to reuse a validation copy".into());
        }
        let original = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let version: i64 = original.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        original.execute("VACUUM INTO ?1", [&path])?;
        let mut lib = Library::open(&path)?;
        let before = lib.connection_coverage("musicbrainz")?;
        let sql = Connection::open(&path)?;
        let groups = lib.connection_groups("musicbrainz", false, None, 200)?;
        let mut rows = vec![];
        for g in groups {
            let album = &g.id;
            let ids = lib.list_album_external_identities(album)?;
            let trusted = ids
                .iter()
                .find(|i| i.provider == "musicbrainz" && i.kind == "release_group")
                .cloned();
            let observations = evidence["probes"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|d| {
                    d["title"].as_str().or(d["request"]["title"].as_str()) == Some(g.title.as_str())
                        && !d["programs"].is_null()
                })
                .collect::<Vec<_>>();
            let mut auto_group = trusted.clone();
            let mut plans = vec![];
            for probe in observations {
                let p: Programs = serde_json::from_value(probe["programs"].clone())?;
                lib.retain_musicbrainz_programs(album, &p)?;
                let local = lib.local_album_tracks(album)?;
                let a = music_library::musicbrainz_recording::assess(&local, &p, &g.title);
                for (n, raw) in p.programs.iter().enumerate() {
                    assert_eq!(
                        serde_json::to_value(&a.programs[n])?,
                        serde_json::to_value(pc::correspond(&local, raw, &g.title))?
                    );
                }
                // These candidates were already returned by the pre-change bounded searches.
                // Replay that existing evaluation; do not discover or associate native-title Groups.
                let existing_title_candidate = probe.get("automatic_candidates").is_some()
                    || probe.get("artist_page").is_some();
                if auto_group.is_none() && existing_title_candidate {
                    if let Preparation::Ready(input) = lib.prepare_album_match(album)? {
                        let primary = p.programs[0].tracks[0].artists.first().unwrap();
                        let candidate_artist = primary
                            .identities
                            .iter()
                            .find(|i| i.provider == "musicbrainz" && i.kind == "artist")
                            .unwrap()
                            .clone();
                        let artist_page = Page {
                            items: vec![ArtistCandidate {
                                identity: candidate_artist.clone(),
                                name: primary.name.clone(),
                                aliases: vec![],
                                country: String::new(),
                                comment: String::new(),
                                artist_type: String::new(),
                                score: None,
                            }],
                            next_offset: None,
                        };
                        if let Ok(artist) =
                            input.known_artist.clone().map(Ok).unwrap_or_else(|| {
                                music_library::album_matching::resolve_artist(
                                    &input.artist,
                                    &artist_page,
                                )
                            })
                        {
                            let raw = probe["manual_page"].as_str().unwrap_or("");
                            let title = raw
                                .split("title: \"")
                                .nth(1)
                                .and_then(|s| s.split('"').next())
                                .unwrap_or(&g.title)
                                .to_string();
                            let candidate = ArtistAlbumCandidate {
                                identity: p.album.clone(),
                                title,
                                artist: primary.name.clone(),
                                artist_ids: vec![candidate_artist],
                                date: String::new(),
                                primary_type: "EP".into(),
                                comment: String::new(),
                            };
                            let page = Page {
                                items: vec![candidate.clone()],
                                next_offset: None,
                            };
                            let mut provider = Retained(p.clone());
                            let mut cache = vec![];
                            let resolution = music_library::album_candidates::resolve_canonical(
                                &mut provider,
                                (&input.raw_title, input.date),
                                &artist,
                                &page,
                                false,
                                &local,
                                &mut cache,
                                &input.equivalent_artists,
                                &input.evidence,
                            )?;
                            let outcome = lib.complete_album_match(MatchReply {
                                input,
                                artist: Some(artist),
                                artist_observation: None,
                                outcome: resolution.outcome,
                                matched_album: Some(candidate),
                                diagnostic: Some(resolution.report),
                            })?;
                            if matches!(
                                outcome,
                                MatchOutcome::Matched(_) | MatchOutcome::MatchedClose(_)
                            ) {
                                auto_group = Some(p.album.clone());
                            }
                        }
                    }
                }
                let local = lib.local_album_tracks(album)?;
                let a = music_library::musicbrainz_recording::assess(&local, &p, &g.title);
                let summary = json!({"group":p.album,"structural":a.programs.iter().map(|c|c.program.tracks.iter().filter(|r|r.decision.is_correspondence()).count()).collect::<Vec<_>>(),"complete":p.programs.iter().map(|p|p.complete).collect::<Vec<_>>(),"incomplete_samples":a.incomplete_samples,"certainty":a.recordings});
                plans.push(summary);
                if auto_group.as_ref() == Some(&p.album) {
                    let _ = lib.evaluate_retained_musicbrainz(album, &p.album)?;
                }
            }
            if trusted.is_some() {
                assert_eq!(
                    lib.list_album_external_identities(album)?,
                    ids,
                    "must not replace existing Group: {}",
                    g.title
                );
            }
            let after_tracks = lib.connection_tracks("musicbrainz", false, album, None, 200)?;
            let lifecycle = lib.musicbrainz_review(album)?;
            let lifecycle = json!({"reason":lifecycle.reason,"stale":lifecycle.stale,"candidates":lifecycle.candidates.iter().map(|c|json!({"group":c.metadata.identity,"title":c.metadata.title,"origin":c.origin,"reason":c.reason,"observed_at":c.observed_at,"program_observed_at":c.program_observed_at,"sampled_releases":c.programs.as_ref().map(|p|p.programs.iter().map(|p|json!({"release":p.identity,"complete":p.complete,"tracks":p.tracks.len()})).collect::<Vec<_>>())})).collect::<Vec<_>>()});
            rows.push(json!({"album":g.title,"artist":g.artist,"saved":g.total,"before_connected":g.matched,"after_connected":g.total-after_tracks.len() as i64,"plans":plans,"remaining":after_tracks.iter().map(|t|json!({"title":t.title,"reason":t.reason})).collect::<Vec<_>>(),"lifecycle":lifecycle}));
        }
        let after = lib.connection_coverage("musicbrainz")?;
        assert_eq!(before.saved, after.saved);
        let schema: i64 = sql.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        assert_eq!(schema, 38);
        // New process/connection boundary: observations and programs remain accessible offline.
        drop(lib);
        let lib = Library::open(&path)?;
        for g in lib.connection_groups("musicbrainz", false, None, 200)? {
            let _ = lib.musicbrainz_review(&g.id)?;
        }
        reports.push(json!({"dataset":name,"source_schema":version,"schema":schema,"path":path,"before":before,"after":after,"albums":rows,"provider_requests":0}));
    }
    println!("{}", serde_json::to_string_pretty(&reports)?);
    Ok(())
}
