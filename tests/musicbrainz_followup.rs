use music_library::{
    Library,
    album_program::{PositionDecision, Program, Programs},
    artist_credit::{name_key, same_artist},
    catalog::*,
    domain::*,
    edition::*,
    musicbrainz_lifecycle::Reason,
    musicbrainz_recording::RecordingCertainty as C,
    program_correspondence::{self as pc},
};
fn id(kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "musicbrainz".into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
fn artist(name: &str, value: &str) -> ArtistEvidence {
    ArtistEvidence {
        name: name.into(),
        identities: vec![id("artist", value)],
        join_phrase: String::new(),
    }
}
fn local(names: &[&str]) -> Vec<LocalTrackEvidence> {
    names
        .iter()
        .enumerate()
        .map(|(i, t)| LocalTrackEvidence {
            track_id: TrackId(format!("t{i}")),
            recording_id: RecordingId(format!("r{i}")),
            evidence: TrackEvidence {
                title: Some(t.to_string()),
                number: Some(i as u32 + 1),
                disc: Some(1),
                artists: vec![artist("Band", "band")],
                duration_ms: Some(100000),
                ..Default::default()
            },
        })
        .collect()
}
fn program(names: &[&str]) -> Program {
    Program {
        identity: Some(id("release", "release")),
        tracks: local(names)
            .into_iter()
            .enumerate()
            .map(|(i, mut l)| {
                l.evidence.recording.identities = vec![id("recording", &format!("rec{i}"))];
                l.evidence
            })
            .collect(),
        complete: true,
    }
}
fn programs(p: Vec<Program>) -> Programs {
    Programs {
        album: id("release_group", "group"),
        programs: p,
        note: "bounded fixture".into(),
    }
}
#[test]
fn artist_typography_is_full_name_and_conflicts_veto() {
    assert_eq!(name_key("Gumby's Junk"), name_key("Gumby’s Junk"));
    assert_eq!(name_key("E\u{301}CHO"), name_key("écho"));
    assert_ne!(name_key("Hop Along"), name_key("Hop Along, Queen Ansleis"));
    assert!(!same_artist(&artist("Band", "one"), &artist("Band", "two")));
    let page = Page {
        items: vec![ArtistCandidate {
            identity: id("artist", "g"),
            name: "Gumby’s Junk".into(),
            aliases: vec![],
            comment: String::new(),
            country: String::new(),
            artist_type: String::new(),
            score: Some(100),
        }],
        next_offset: None,
    };
    assert!(music_library::album_matching::resolve_artist("Gumby's Junk", &page).is_ok());
    let mut alias = page.clone();
    alias.items[0].aliases = vec!["Old Band".into()];
    assert!(music_library::album_matching::resolve_artist("Old Band", &alias).is_ok());
    alias.items.push(ArtistCandidate {
        identity: id("artist", "other"),
        ..alias.items[0].clone()
    });
    assert!(music_library::album_matching::resolve_artist("Old Band", &alias).is_err());
}
#[test]
fn musicbrainz_context_supports_placeholder_and_token_boundary() {
    for (a, b) in [("_", "-"), ("OCDC", "OC/DC")] {
        let l = local(&[a, "Second", "Third", "Fourth", "Fifth"]);
        let p = program(&[b, "Second", "Third", "Fourth", "Fifth"]);
        let observed = pc::correspond(&l, &p, "Album");
        assert!(
            observed.program.tracks[0].decision.is_correspondence(),
            "{observed:?}"
        );
        let a = music_library::musicbrainz_recording::assess(&l, &programs(vec![p]), "Album");
        assert_eq!(a.recordings[0].certainty, C::Supported);
        assert_eq!(
            serde_json::to_value(&a.programs[0]).unwrap(),
            serde_json::to_value(observed).unwrap()
        );
    }
}
#[test]
fn complete_agreement_incomplete_uncertainty_and_conflicts_are_separate() {
    let l = local(&["One", "Two", "Three", "Four"]);
    let a = program(&["One", "Two", "Three", "Four"]);
    let mut partial = a.clone();
    partial.complete = false;
    partial.tracks.truncate(1);
    let result = music_library::musicbrainz_recording::assess(
        &l,
        &programs(vec![a.clone(), a.clone(), partial.clone()]),
        "Album",
    );
    assert_eq!(result.incomplete_samples, 1);
    assert!(
        result
            .recordings
            .iter()
            .all(|r| r.certainty == C::Supported)
    );
    let mut competing = a.clone();
    competing.tracks[0].recording.identities = vec![id("recording", "different")];
    assert_eq!(
        music_library::musicbrainz_recording::assess(
            &l,
            &programs(vec![a.clone(), competing]),
            "Album"
        )
        .recordings[0]
            .certainty,
        C::RecordingAmbiguous
    );
    assert_eq!(
        music_library::musicbrainz_recording::confirmation(&pc::correspond(&l, &a, "Album")).len(),
        4
    );
    assert!(
        music_library::musicbrainz_recording::assess(&l, &programs(vec![partial]), "Album")
            .recordings
            .iter()
            .all(|r| r.recording.is_none())
    );
    let mut known = l.clone();
    known[0].evidence.recording.identities = vec![id("recording", "wrong")];
    assert!(
        music_library::musicbrainz_recording::assess(&known, &programs(vec![a]), "Album")
            .recordings[0]
            .recording
            .is_none()
    );
}
#[test]
fn insertion_missing_reordering_and_multidisc_have_one_structural_result() {
    let l = local(&["One", "Two", "Three", "Extra", "Four", "Five"]);
    let p = program(&["One", "Two", "Three", "Four", "Five"]);
    let c = pc::correspond(&l, &p, "Album");
    assert_eq!(c.program.tracks[3].decision, PositionDecision::LocalOnly);
    assert!(c.program.tracks[4].decision.is_correspondence());
    let l = local(&["One", "Two", "Three", "Four", "Five"]);
    let p = program(&["Five", "One", "Two", "Three", "Four", "Provider only"]);
    let c = pc::correspond(&l, &p, "Album");
    assert!(
        c.program
            .tracks
            .iter()
            .all(|r| r.decision.is_correspondence())
    );
    assert_eq!(c.provider_only.len(), 1);
    let mut l = local(&["One", "Two", "Three", "Four"]);
    let mut p = program(&["One", "Two", "Three", "Four"]);
    for (i, t) in l.iter_mut().enumerate() {
        t.evidence.disc = Some(i as u32 / 2 + 1);
        t.evidence.number = Some(i as u32 % 2 + 1);
    }
    for (i, t) in p.tracks.iter_mut().enumerate() {
        t.disc = l[i].evidence.disc;
        t.number = l[i].evidence.number;
    }
    assert!(
        pc::correspond(&l, &p, "Album")
            .program
            .tracks
            .iter()
            .all(|r| r.decision.is_correspondence())
    );
    for t in &mut l {
        t.evidence.number = Some(9999)
    }
    assert!(
        pc::correspond(&l, &p, "Album")
            .program
            .tracks
            .iter()
            .all(|r| r.decision.is_correspondence())
    );
}
#[test]
fn annotations_need_credits_and_remaster_correspondence_is_not_recording_identity() {
    let l = local(&["One (feat. Guest)", "Two", "Three", "Four"]);
    let mut p = program(&["One", "Two", "Three", "Four"]);
    p.tracks[0].artists.push(artist("Guest", "guest"));
    assert!(
        pc::correspond(&l, &p, "Album").program.tracks[0]
            .decision
            .is_correspondence()
    );
    assert_eq!(
        music_library::musicbrainz_recording::assess(&l, &programs(vec![p.clone()]), "Album")
            .recordings[0]
            .certainty,
        C::Supported
    );
    p.tracks[0].artists.pop();
    assert_ne!(
        music_library::musicbrainz_recording::assess(&l, &programs(vec![p]), "Album").recordings[0]
            .certainty,
        C::Supported
    );
    let l = local(&[
        "One (2015 Remaster)",
        "Two (2015 Remaster)",
        "Three (2015 Remaster)",
        "Four (2015 Remaster)",
    ]);
    let p = program(&["One", "Two", "Three", "Four"]);
    let c = pc::correspond(&l, &p, "Album (2015 Remaster)");
    assert!(
        c.program
            .tracks
            .iter()
            .all(|r| r.decision.is_correspondence())
    );
    assert!(
        music_library::musicbrainz_recording::assess(
            &l,
            &programs(vec![p]),
            "Album (2015 Remaster)"
        )
        .recordings
        .iter()
        .all(|r| r.recording.is_none())
    );
    for version in ["live", "acoustic", "demo", "remix"] {
        let l = local(&[&format!("One ({version})"), "Two", "Three", "Four"]);
        let p = program(&["One", "Two", "Three", "Four"]);
        assert!(
            music_library::musicbrainz_recording::assess(&l, &programs(vec![p]), "Album")
                .recordings[0]
                .recording
                .is_none()
        );
    }
}
#[test]
fn shared_title_is_script_neutral_and_contextual() {
    let l = local(&["Mirror", "Midway", "D", "Spring Rain", "Celebrate"]);
    let p = program(&[
        "Mirror 鏡子",
        "Midway 中途",
        "D",
        "Spring Rain 春雨",
        "Celebrate 慶",
    ]);
    assert!(
        pc::correspond(&l, &p, "Work")
            .program
            .tracks
            .iter()
            .all(|r| r.decision.is_correspondence())
    );
    let mut l = local(&["Water", "Two", "Three", "Four"]);
    l[0].evidence.title = Some("水中漫步 Underwater".into());
    let p = program(&["Underwater", "Two", "Three", "Four"]);
    assert!(
        pc::correspond(&l, &p, "Album").program.tracks[0]
            .decision
            .is_correspondence()
    );
}
fn library(path: &std::path::Path) -> (Library, AlbumId, Vec<TrackId>) {
    let mut l = Library::open(path).unwrap();
    let r = l
        .create_catalog_release(&CatalogReleaseInput {
            title: "Album".into(),
            year: None,
            artists: vec![ArtistCreditInput {
                name: "Band".into(),
                role: None,
            }],
            tracks: (1..=4)
                .map(|n| CatalogTrackInput {
                    title: format!("Track {n}"),
                    disc_number: Some(1),
                    track_number: Some(n),
                    artists: vec![],
                })
                .collect(),
        })
        .unwrap();
    for t in &r.track_ids {
        l.add_to_library(t).unwrap();
    }
    let a = l.album_id_for_track(&r.track_ids[0]).unwrap();
    (l, a, r.track_ids)
}
fn metadata() -> AlbumCandidate {
    AlbumCandidate {
        identity: id("release_group", "group"),
        title: "Album".into(),
        artist: "Band".into(),
        credits: vec![Credit {
            identity: Some(id("artist", "band")),
            name: "Band".into(),
            join_phrase: String::new(),
        }],
        date: "2000".into(),
        primary_type: "Album".into(),
        secondary_types: vec![],
        score: Some(99),
        comment: String::new(),
    }
}
#[test]
fn durable_candidates_states_programs_and_invalidation_without_identity_writes() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("library");
    let (mut l, a, tracks) = library(&path);
    assert_eq!(
        l.musicbrainz_review(&a).unwrap().reason,
        Reason::NotAttempted
    );
    l.retain_musicbrainz_candidates(&a, &[], false).unwrap();
    assert_eq!(
        l.musicbrainz_review(&a).unwrap().reason,
        Reason::NoCandidates
    );
    l.retain_musicbrainz_candidates(&a, &[metadata()], true)
        .unwrap();
    assert_eq!(
        l.musicbrainz_review(&a).unwrap().reason,
        Reason::CandidatesFound
    );
    let pg = programs(vec![program(&["Track 1", "Track 2", "Track 3", "Track 4"])]);
    l.retain_musicbrainz_programs(&a, &pg).unwrap();
    let state_json: String = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT track_reasons FROM musicbrainz_match_state WHERE album_id=?1",
            [a.as_ref()],
            |row| row.get(0),
        )
        .unwrap();
    let reasons: serde_json::Value = serde_json::from_str(&state_json).unwrap();
    assert_eq!(reasons[&tracks[0].0]["code"], "artist_unresolved");
    assert_eq!(
        l.connection_tracks("musicbrainz", false, &a, None, 200)
            .unwrap()[0]
            .reason,
        Reason::ArtistUnresolved.label()
    );
    assert!(l.list_album_external_identities(&a).unwrap().is_empty());
    drop(l);
    let mut l = Library::open(&path).unwrap();
    let r = l.musicbrainz_review(&a).unwrap();
    assert!(!r.stale);
    assert_eq!(r.candidates[0].programs.as_ref(), Some(&pg));
    assert_eq!(r.candidates[0].metadata.score, Some(99));
    assert_eq!(r.candidates[0].origin, "manual");
    assert!(
        l.evaluate_retained_musicbrainz(&a, &id("release_group", "group"))
            .is_err()
    );
    for reason in [
        Reason::AlbumTitleConflict,
        Reason::ProviderDeferred,
        Reason::ProviderError,
        Reason::ProgramIncomplete,
    ] {
        l.musicbrainz_state(&a, reason).unwrap();
        assert_eq!(l.musicbrainz_review(&a).unwrap().reason, reason);
        assert_ne!(
            l.connection_tracks("musicbrainz", false, &a, None, 200)
                .unwrap()[0]
                .reason,
            "No MusicBrainz Recording"
        );
    }
    l.set_track_title_override(&tracks[0], "Changed").unwrap();
    assert_eq!(
        l.musicbrainz_review(&a).unwrap().reason,
        Reason::EvidenceChanged
    );
    assert!(l.musicbrainz_review(&a).unwrap().stale);
    assert_eq!(l.connection_coverage("musicbrainz").unwrap().saved, 4);
    l.attach_album_external_identity(&a, &id("release_group", "original"))
        .unwrap();
    l.retain_musicbrainz_programs(&a, &pg).unwrap();
    assert_eq!(
        l.list_album_external_identities(&a).unwrap(),
        vec![id("release_group", "original")]
    );
}
#[test]
fn retained_programs_and_lists_are_bounded() {
    let t = tempfile::tempdir().unwrap();
    let (mut l, a, _) = library(&t.path().join("library"));
    for n in 0..45 {
        let mut c = metadata();
        c.identity.external_id = format!("g{n:02}");
        l.retain_musicbrainz_candidates(&a, &[c], true).unwrap();
    }
    assert_eq!(l.musicbrainz_review(&a).unwrap().candidates.len(), 40);
    assert!(
        l.connection_groups("musicbrainz", false, None, u32::MAX)
            .unwrap()
            .len()
            <= 200
    );
    assert!(
        l.connection_tracks("musicbrainz", false, &a, None, u32::MAX)
            .unwrap()
            .len()
            <= 200
    );
}
#[test]
fn all_retained_audit_programs_share_exact_structure_and_known_controls() {
    let evidence: serde_json::Value = serde_json::from_str(include_str!(
        "../docs/musicbrainz-matching-audit-evidence.json"
    ))
    .unwrap();
    let mut inspected = 0;
    for probe in evidence["probes"].as_array().unwrap() {
        let title = probe["title"]
            .as_str()
            .or(probe["request"]["title"].as_str())
            .unwrap_or("");
        if probe["programs"].is_null() {
            continue;
        }
        let source = evidence["probes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["title"] == title && p["local"].is_array());
        let Some(source) = source else { continue };
        let l = source["local"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| LocalTrackEvidence {
                track_id: serde_json::from_value(r["track_id"].clone()).unwrap(),
                recording_id: RecordingId("audit".into()),
                evidence: serde_json::from_value(r["evidence"].clone()).unwrap(),
            })
            .collect::<Vec<_>>();
        let pg: Programs = serde_json::from_value(probe["programs"].clone()).unwrap();
        let a = music_library::musicbrainz_recording::assess(&l, &pg, title);
        for (i, p) in pg.programs.iter().enumerate() {
            assert_eq!(
                serde_json::to_value(&a.programs[i]).unwrap(),
                serde_json::to_value(pc::correspond(&l, p, title)).unwrap()
            );
        }
        if [
            "Freshman Year",
            "100 Onces",
            "Work",
            "Underwater",
            "The Devil Isn't Red",
        ]
        .contains(&title)
        {
            let n = a.programs[0]
                .program
                .tracks
                .iter()
                .filter(|r| r.decision.is_correspondence())
                .count();
            assert_eq!(n, l.len(), "{title}");
        }
        if title == "The Devil Isn't Red" {
            assert_eq!(a.incomplete_samples, 1);
            assert!(
                a.recordings.iter().all(|r| r.recording.is_some()),
                "{:?}",
                a.recordings
            );
        }
        inspected += 1;
    }
    assert!(inspected >= 13);
}
struct CacheProvider(std::sync::Arc<std::sync::atomic::AtomicUsize>);
impl CatalogProvider for CacheProvider {
    fn album_program_namespaces(&self) -> Vec<(String, String)> {
        vec![("musicbrainz".into(), "release_group".into())]
    }
    fn album_programs(&mut self, _: &ExternalIdentity) -> Result<Programs, CatalogError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(CatalogError::Other("unexpected network attempt".into()))
    }
    fn search_albums(&mut self, _: &str, _: u32) -> Result<Page<AlbumCandidate>, CatalogError> {
        panic!("unexpected provider request")
    }
    fn releases(
        &mut self,
        _: &ExternalIdentity,
        _: u32,
    ) -> Result<Page<ReleaseCandidate>, CatalogError> {
        panic!("unexpected provider request")
    }
    fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
        panic!("unexpected provider request")
    }
}
#[test]
fn restarted_automatic_worker_reuses_program_with_zero_requests_and_explicit_refresh_bypasses() {
    use music_library::album_matching::AlbumMatcher;
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("db");
    let (mut l, a, _) = library(&path);
    l.attach_album_external_identity(&a, &id("release_group", "group"))
        .unwrap();
    l.retain_musicbrainz_programs(
        &a,
        &programs(vec![program(&["Track 1", "Track 2", "Track 3", "Track 4"])]),
    )
    .unwrap();
    drop(l);
    let mut l = Library::open(path).unwrap();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (send, receive) = std::sync::mpsc::channel();
    let mut m = AlbumMatcher::new_with_programs(
        CacheProvider(calls.clone()),
        |_| panic!("no discovery needed"),
        move |r| send.send(r).unwrap(),
        |_| {},
    )
    .unwrap();
    m.match_album(&l, &a).unwrap();
    let reply = receive
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    m.complete_programs(&mut l, reply);
    m.refresh_album(&l, &a).unwrap();
    let reply = receive
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(reply.result.is_err());
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}
#[test]
fn migration_from_audit_versions_preserves_identities_membership_spotify_and_choices() {
    for version in [33, 36] {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("db");
        let (mut l, a, tracks) = library(&path);
        l.attach_album_external_identity(&a, &id("release_group", "group"))
            .unwrap();
        l.mark_connection_unavailable("spotify", None, Some(&tracks[0]))
            .unwrap();
        l.mark_connection_unavailable("musicbrainz", None, Some(&tracks[1]))
            .unwrap();
        drop(l);
        let db = rusqlite::Connection::open(&path).unwrap();
        let triggers = db
            .prepare("SELECT name FROM sqlite_master WHERE type='trigger' AND name LIKE 'mb_%'")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        for name in triggers {
            db.execute_batch(&format!("DROP TRIGGER {name}")).unwrap();
        }
        db.execute_batch("DROP TABLE musicbrainz_candidate; DROP TABLE musicbrainz_match_state;")
            .unwrap();
        db.pragma_update(None, "user_version", version).unwrap();
        drop(db);
        let l = Library::open(path).unwrap();
        assert_eq!(l.connection_coverage("musicbrainz").unwrap().saved, 4);
        assert_eq!(l.connection_coverage("spotify").unwrap().marked, 1);
        assert_eq!(l.connection_coverage("musicbrainz").unwrap().marked, 1);
        assert_eq!(
            l.list_album_external_identities(&a).unwrap(),
            vec![id("release_group", "group")]
        );
        assert_eq!(
            l.musicbrainz_review(&a).unwrap().reason,
            Reason::NotAttempted
        );
    }
}
#[test]
fn artist_and_membership_changes_stale_observations_and_interrupted_runs_do_not_claim_matching() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("db");
    let (mut l, a, tracks) = library(&path);
    l.retain_musicbrainz_candidates(&a, &[metadata()], true)
        .unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("INSERT INTO artist_external_identity SELECT artist_id,'musicbrainz','artist','new-artist' FROM album_artist_credit WHERE album_id=?1",[a.as_ref()]).unwrap();
    assert_eq!(
        l.musicbrainz_review(&a).unwrap().reason,
        Reason::EvidenceChanged
    );
    l.musicbrainz_state(&a, Reason::Matching).unwrap();
    db.execute(
        "UPDATE musicbrainz_match_state SET run_id='old-process'",
        [],
    )
    .unwrap();
    assert!(l.musicbrainz_review(&a).unwrap().stale);
    l.musicbrainz_state(&a, Reason::CandidatesFound).unwrap();
    l.remove_from_library(&tracks[0]).unwrap();
    assert_eq!(
        l.musicbrainz_review(&a).unwrap().reason,
        Reason::EvidenceChanged
    );
}
#[test]
fn structural_duration_support_does_not_copy_playback_recording_tolerance() {
    let l = local(&["One", "Two", "Three", "Four"]);
    let mut p = program(&["One", "Two", "Three", "Four"]);
    p.tracks[0].duration_ms = Some(106000);
    assert!(
        pc::correspond(&l, &p, "Album").program.tracks[0]
            .decision
            .is_correspondence()
    );
    assert_eq!(
        music_library::musicbrainz_recording::assess(&l, &programs(vec![p]), "Album").recordings[0]
            .certainty,
        C::DurationConflict
    );
}
#[test]
fn provider_observation_ttl_does_not_renew_merely_because_review_opens() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("db");
    let (mut l, a, _) = library(&path);
    let p = programs(vec![program(&["Track 1", "Track 2", "Track 3", "Track 4"])]);
    l.retain_musicbrainz_programs(&a, &p).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE musicbrainz_candidate SET program_observed_at=unixepoch()-3000000",
        [],
    )
    .unwrap();
    let r = l.musicbrainz_review(&a).unwrap();
    assert!(r.stale);
    let at = r.candidates[0].program_observed_at;
    let r = l.musicbrainz_review(&a).unwrap();
    assert_eq!(r.candidates[0].program_observed_at, at);
    assert!(r.candidates[0].programs.is_some());
}
#[test]
fn explicit_inspection_never_replaces_automatic_sample_agreement() {
    let t = tempfile::tempdir().unwrap();
    let (mut l, a, _) = library(&t.path().join("db"));
    let p = program(&["Track 1", "Track 2", "Track 3", "Track 4"]);
    let mut competing = p.clone();
    competing.identity = Some(id("release", "other-release"));
    competing.tracks[0].recording.identities = vec![id("recording", "other")];
    let ensemble = programs(vec![p.clone(), competing]);
    l.retain_musicbrainz_programs(&a, &ensemble).unwrap();
    let inspection = programs(vec![p]);
    l.retain_musicbrainz_inspection(&a, &inspection).unwrap();
    let r = l.musicbrainz_review(&a).unwrap();
    let c = &r.candidates[0];
    assert_eq!(c.programs.as_ref(), Some(&ensemble));
    assert_eq!(c.review_programs(), Some(&inspection));
    let local = local(&["Track 1", "Track 2", "Track 3", "Track 4"]);
    assert_eq!(
        music_library::musicbrainz_recording::assess(&local, c.programs.as_ref().unwrap(), "Album")
            .recordings[0]
            .certainty,
        C::RecordingAmbiguous
    );
}

mod structured_discovery {
    use super::*;
    use music_library::{
        album_matching::{MatchInput, MatchOutcome},
        musicbrainz_discovery as d,
    };
    fn input() -> MatchInput {
        MatchInput {
            evidence: Box::default(),
            date: None,
            album_id: AlbumId("album".into()),
            title: "Work".into(),
            raw_title: "Work".into(),
            artist: "Band".into(),
            artist_id: ArtistId("band".into()),
            known_artist: Some(id("artist", "band")),
            equivalent_artists: vec![],
            manual_artist: false,
        }
    }
    #[derive(Default)]
    struct Provider {
        pages: Vec<Page<AlbumCandidate>>,
        query: Vec<AlbumCandidate>,
        programs: Vec<Programs>,
        calls: Vec<String>,
    }
    impl CatalogProvider for Provider {
        fn structured_group_discovery(&self) -> bool {
            true
        }
        fn album_program_namespaces(&self) -> Vec<(String, String)> {
            vec![("musicbrainz".into(), "release_group".into())]
        }
        fn search_albums(&mut self, _: &str, _: u32) -> Result<Page<AlbumCandidate>, CatalogError> {
            panic!("no global query")
        }
        fn releases(
            &mut self,
            _: &ExternalIdentity,
            _: u32,
        ) -> Result<Page<ReleaseCandidate>, CatalogError> {
            panic!("no per-Track or exact Release work")
        }
        fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
            panic!()
        }
        fn artist_albums(
            &mut self,
            a: &ExternalIdentity,
            _: &str,
        ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
            assert_eq!(a, &id("artist", "band"));
            self.calls.push("title".into());
            Ok(Page {
                items: self.query.iter().map(d::artist_candidate).collect(),
                next_offset: None,
            })
        }
        fn album_candidate_observation(&self, a: &ExternalIdentity) -> Option<AlbumCandidate> {
            self.query.iter().find(|c| &c.identity == a).cloned()
        }
        fn artist_release_groups(
            &mut self,
            a: &ExternalIdentity,
            offset: u32,
        ) -> Result<Page<AlbumCandidate>, CatalogError> {
            assert_eq!(a, &id("artist", "band"));
            self.calls.push(format!("page:{offset}"));
            Ok(self
                .pages
                .get(offset as usize / 10)
                .cloned()
                .unwrap_or(Page {
                    items: vec![],
                    next_offset: None,
                }))
        }
        fn album_programs(&mut self, a: &ExternalIdentity) -> Result<Programs, CatalogError> {
            self.calls.push(format!("program:{}", a.external_id));
            Ok(self
                .programs
                .iter()
                .find(|p| &p.album == a)
                .cloned()
                .unwrap_or(Programs {
                    album: a.clone(),
                    programs: vec![],
                    note: String::new(),
                }))
        }
    }
    fn candidate(title: &str, group: &str) -> AlbumCandidate {
        let mut c = metadata();
        c.title = title.into();
        c.identity = id("release_group", group);
        c
    }
    fn pg(group: &str, names: &[&str]) -> Programs {
        let mut p = programs(vec![program(names)]);
        p.album = id("release_group", group);
        p
    }
    const NAMES: [&str; 5] = ["One", "Two", "Three", "Four", "Five"];
    #[test]
    fn trusted_scope_native_third_page_and_zero_request_replay() {
        let mut p = Provider {
            pages: vec![
                Page {
                    items: vec![],
                    next_offset: Some(10),
                },
                Page {
                    items: vec![],
                    next_offset: Some(20),
                },
                Page {
                    items: vec![candidate("工作", "work")],
                    next_offset: None,
                },
            ],
            programs: vec![pg("work", &NAMES)],
            ..Default::default()
        };
        let r = d::discover(
            &mut p,
            &input(),
            &id("artist", "band"),
            &local(&NAMES),
            None,
        );
        assert!(
            matches!(r.outcome, MatchOutcome::MatchedClose(_)),
            "{:?}",
            r.report
        );
        assert_eq!(
            p.calls,
            vec!["title", "page:0", "page:10", "page:20", "program:work"]
        );
        p.calls.clear();
        let r = d::discover(
            &mut p,
            &input(),
            &id("artist", "band"),
            &local(&NAMES),
            r.report.musicbrainz_discovery,
        );
        assert!(matches!(r.outcome, MatchOutcome::MatchedClose(_)));
        assert!(p.calls.is_empty());
    }
    #[test]
    fn stops_on_unique_strong_candidate_and_not_provider_rank() {
        let mut wrong = candidate("Work", "wrong");
        wrong.credits[0].identity = Some(id("artist", "unrelated"));
        let mut p = Provider {
            query: vec![wrong, candidate("Work", "work")],
            programs: vec![pg("work", &NAMES)],
            ..Default::default()
        };
        let r = d::discover(
            &mut p,
            &input(),
            &id("artist", "band"),
            &local(&NAMES),
            None,
        );
        assert_eq!(p.calls, vec!["title", "program:work"]);
        assert!(matches!(r.outcome, MatchOutcome::MatchedClose(_)));
    }
    #[test]
    fn native_title_without_program_never_associates_and_weak_overlap_fails() {
        for titles in [
            vec!["Unrelated", "Other", "Unknown", "Four", "Five"],
            vec!["One", "Two", "Unknown", "Other", "Another"],
        ] {
            let e = d::evaluate(
                &input(),
                &candidate("工作", "work"),
                &id("artist", "band"),
                &local(&NAMES),
                &pg("work", &titles),
            );
            assert!(!e.strong);
            assert!(e.reasons.contains(&"insufficient_program_anchors".into()));
        }
    }
    #[test]
    fn competing_programs_need_review() {
        let mut p = Provider {
            query: vec![candidate("工作", "one"), candidate("Other", "two")],
            programs: vec![pg("one", &NAMES), pg("two", &NAMES)],
            ..Default::default()
        };
        let r = d::discover(
            &mut p,
            &input(),
            &id("artist", "band"),
            &local(&NAMES),
            None,
        );
        assert!(matches!(r.outcome, MatchOutcome::AlbumAmbiguous(_)));
        assert_eq!(
            r.report.reasons,
            vec![music_library::album_candidates::Reason::CompetingCandidates]
        );
    }
    #[test]
    fn page_budget_differs_from_completed_empty() {
        let mut p = Provider {
            pages: (0..4)
                .map(|n| Page {
                    items: vec![],
                    next_offset: Some((n + 1) * 10),
                })
                .collect(),
            ..Default::default()
        };
        let r = d::discover(
            &mut p,
            &input(),
            &id("artist", "band"),
            &local(&NAMES),
            None,
        );
        assert_eq!(p.calls.len(), 4);
        assert!(r.report.musicbrainz_discovery.unwrap().budget_exhausted);
        let mut p = Provider::default();
        let r = d::discover(
            &mut p,
            &input(),
            &id("artist", "band"),
            &local(&NAMES),
            None,
        );
        assert!(!r.report.musicbrainz_discovery.unwrap().budget_exhausted);
        assert_eq!(
            r.report.reasons,
            vec![music_library::album_candidates::Reason::NoCandidates]
        );
    }
    #[test]
    fn program_budget_and_uninspected_competitors_block_association() {
        let mut p = Provider {
            query: (0..10)
                .map(|n| candidate("Work", &format!("g{n}")))
                .collect(),
            programs: (0..10).map(|n| pg(&format!("g{n}"), &NAMES)).collect(),
            ..Default::default()
        };
        let r = d::discover(
            &mut p,
            &input(),
            &id("artist", "band"),
            &local(&NAMES),
            None,
        );
        let s = r.report.musicbrainz_discovery.unwrap();
        assert!(s.budget_exhausted);
        assert_eq!(s.programs.len(), d::MAX_GROUP_PROGRAMS);
        assert!(s.requests_upper_bound <= d::MAX_REQUESTS);
        assert!(!matches!(
            r.outcome,
            MatchOutcome::Matched(_) | MatchOutcome::MatchedClose(_)
        ));
    }
    #[test]
    fn remaster_and_existing_trusted_group_are_vetoes() {
        let mut i = input();
        i.raw_title = "Work (2015 Remaster)".into();
        let e = d::evaluate(
            &i,
            &candidate("Work", "work"),
            &id("artist", "band"),
            &local(&NAMES),
            &pg("work", &NAMES),
        );
        assert!(!e.strong);
        assert!(e.reasons.contains(&"version_conflict".into()));
        let mut i = input();
        i.evidence
            .titles
            .push(music_library::canonical_evidence::Observation {
                value: "Work".into(),
                origin: music_library::canonical_evidence::Origin::Provider {
                    identity: id("release_group", "trusted"),
                },
            });
        let e = d::evaluate(
            &i,
            &candidate("工作", "work"),
            &id("artist", "band"),
            &local(&NAMES),
            &pg("work", &NAMES),
        );
        assert!(!e.strong);
        assert!(
            e.reasons
                .contains(&"existing_trusted_group_conflict".into())
        );
    }
    #[test]
    fn incomplete_alternative_does_not_poison_complete_program_and_no_exact_release_claim() {
        let mut p = pg("work", &NAMES);
        let mut incomplete = program(&["Other"]);
        incomplete.complete = false;
        p.programs.push(incomplete);
        assert!(
            d::evaluate(
                &input(),
                &candidate("工作", "work"),
                &id("artist", "band"),
                &local(&NAMES),
                &p
            )
            .strong
        );
        assert_eq!(p.album.kind, "release_group");
    }
    #[test]
    fn all_native_controls_are_candidates_without_linguistic_rules() {
        for native in ["工作", "水底", "夢境", "角度"] {
            let mut p = Provider {
                pages: vec![Page {
                    items: vec![candidate(native, "native")],
                    next_offset: None,
                }],
                ..Default::default()
            };
            let r = d::discover(
                &mut p,
                &input(),
                &id("artist", "band"),
                &local(&NAMES),
                None,
            );
            assert!(
                r.report
                    .candidates
                    .iter()
                    .any(|c| c.title.provider_raw == native)
            );
            assert!(!matches!(r.outcome, MatchOutcome::MatchedClose(_)));
        }
    }
    #[test]
    fn extended_provider_title_only_hints_retrieval_and_needs_program() {
        let mut i = input();
        i.raw_title = "Hippy Jam Fest".into();
        i.title = "hippy jam fest".into();
        let c = candidate(
            "Hippy Jam Fest (The Likes of Which Has Never Been Seen)",
            "hippy",
        );
        assert!(d::extended_title(&i.raw_title, &c.title));
        assert_eq!(
            music_library::album_candidates::title_evidence(&i.raw_title, &d::artist_candidate(&c))
                .comparison,
            music_library::album_candidates::TitleComparison::Conflict
        );
        assert!(!d::extended_title("Fest", "Hippy Jam Fest"));
        let mut p = Provider {
            query: vec![c],
            programs: vec![pg("hippy", &NAMES)],
            ..Default::default()
        };
        let r = d::discover(&mut p, &i, &id("artist", "band"), &local(&NAMES), None);
        assert!(matches!(r.outcome, MatchOutcome::MatchedClose(_)));
    }
    #[test]
    fn normal_application_completion_and_reopen_reuse_durable_topology() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db");
        let (mut lib, album, tracks) = library(&path);
        let music_library::album_matching::Preparation::Ready(i) =
            lib.prepare_album_match(&album).unwrap()
        else {
            panic!()
        };
        lib.attach_artist_external_identity(&i.artist_id, &id("artist", "band"))
            .unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        for track in &tracks {
            db.execute(
                "INSERT INTO track_artist_credit(track_id,artist_id,position) VALUES(?1,?2,0)",
                rusqlite::params![track.as_ref(), i.artist_id.as_ref()],
            )
            .unwrap();
        }
        drop(db);
        let mut p = Provider {
            pages: vec![Page {
                items: vec![candidate("工作", "work")],
                next_offset: None,
            }],
            programs: vec![pg("work", &["Track 1", "Track 2", "Track 3", "Track 4"])],
            ..Default::default()
        };
        let r = lib.reconcile_musicbrainz(&album, &mut p).unwrap();
        assert!(
            matches!(r.outcome, MatchOutcome::MatchedClose(_)),
            "{:?}",
            r.report
        );
        assert_eq!(
            lib.list_album_external_identities(&album).unwrap(),
            vec![id("release_group", "work")]
        );
        assert_eq!(lib.connection_coverage("musicbrainz").unwrap().connected, 4);
        assert_eq!(
            lib.connection_coverage("musicbrainz").unwrap().saved,
            tracks.len() as i64
        );
        drop(lib);
        let mut lib = Library::open(&path).unwrap();
        assert_eq!(
            lib.musicbrainz_review(&album).unwrap().candidates[0]
                .metadata
                .title,
            "工作"
        );
        p.calls.clear();
        let _ = lib.reconcile_musicbrainz(&album, &mut p).unwrap();
        assert!(p.calls.is_empty());
        assert!(
            lib.musicbrainz_review(&album).unwrap().candidates[0]
                .programs
                .is_some()
        );
        let db = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row(
                "select count(*) from release_external_identity where provider='musicbrainz'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("select count(*) from musicbrainz_artist_page", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            38
        );
    }
    #[test]
    fn existing_group_alternates_persist_without_replacement() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, album, _) = library(&tmp.path().join("db"));
        let music_library::album_matching::Preparation::Ready(i) =
            lib.prepare_album_match(&album).unwrap()
        else {
            panic!()
        };
        lib.attach_artist_external_identity(&i.artist_id, &id("artist", "band"))
            .unwrap();
        lib.attach_album_external_identity(&album, &id("release_group", "trusted"))
            .unwrap();
        let mut p = Provider {
            pages: vec![Page {
                items: vec![candidate("工作", "alternate")],
                next_offset: None,
            }],
            programs: vec![pg(
                "alternate",
                &["Track 1", "Track 2", "Track 3", "Track 4"],
            )],
            ..Default::default()
        };
        lib.reconcile_musicbrainz(&album, &mut p).unwrap();
        assert_eq!(
            lib.list_album_external_identities(&album).unwrap(),
            vec![id("release_group", "trusted")]
        );
        let review = lib.musicbrainz_review(&album).unwrap();
        assert_eq!(review.reason, Reason::ExistingGroupConflict);
        assert!(
            review
                .candidates
                .iter()
                .any(|c| c.metadata.identity.external_id == "alternate" && c.programs.is_some())
        );
        assert_eq!(lib.connection_coverage("musicbrainz").unwrap().connected, 0);
    }
    #[test]
    fn provider_error_keeps_candidates_and_is_not_completed_empty() {
        struct Unavailable(Provider);
        impl CatalogProvider for Unavailable {
            fn search_albums(
                &mut self,
                _: &str,
                _: u32,
            ) -> Result<Page<AlbumCandidate>, CatalogError> {
                panic!()
            }
            fn releases(
                &mut self,
                _: &ExternalIdentity,
                _: u32,
            ) -> Result<Page<ReleaseCandidate>, CatalogError> {
                panic!()
            }
            fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
                panic!()
            }
            fn artist_albums(
                &mut self,
                a: &ExternalIdentity,
                t: &str,
            ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
                self.0.artist_albums(a, t)
            }
            fn album_candidate_observation(&self, a: &ExternalIdentity) -> Option<AlbumCandidate> {
                self.0.album_candidate_observation(a)
            }
            fn album_programs(&mut self, _: &ExternalIdentity) -> Result<Programs, CatalogError> {
                Err(CatalogError::Timeout("later".into()))
            }
        }
        let mut p = Unavailable(Provider {
            query: vec![candidate("Work", "work")],
            ..Default::default()
        });
        let r = d::discover(
            &mut p,
            &input(),
            &id("artist", "band"),
            &local(&NAMES),
            None,
        );
        assert!(matches!(r.outcome, MatchOutcome::Deferred(_)));
        assert_eq!(r.report.candidates.len(), 1);
        assert!(
            r.report
                .musicbrainz_discovery
                .unwrap()
                .provider_error
                .is_some()
        );
    }
    #[test]
    fn worker_discovers_alternates_for_trusted_partial_album_without_replacement() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, album, _) = library(&tmp.path().join("db"));
        let music_library::album_matching::Preparation::Ready(i) =
            lib.prepare_album_match(&album).unwrap()
        else {
            panic!()
        };
        lib.attach_artist_external_identity(&i.artist_id, &id("artist", "band"))
            .unwrap();
        lib.attach_album_external_identity(&album, &id("release_group", "trusted"))
            .unwrap();
        let p = Provider {
            pages: vec![Page {
                items: vec![candidate("Native", "alternate")],
                next_offset: None,
            }],
            programs: vec![pg(
                "alternate",
                &["Track 1", "Track 2", "Track 3", "Track 4"],
            )],
            ..Default::default()
        };
        let (send, receive) = std::sync::mpsc::channel();
        let (program_send, program_receive) = std::sync::mpsc::channel();
        let mut worker = music_library::album_matching::AlbumMatcher::new_with_programs(
            p,
            move |r| send.send(r).unwrap(),
            move |r| program_send.send(r).unwrap(),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            worker.match_album(&lib, &album).unwrap(),
            MatchOutcome::Pending
        );
        let reply = receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        worker.complete(&mut lib, reply);
        let reply = program_receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        worker.complete_programs(&mut lib, reply);
        assert_eq!(
            lib.list_album_external_identities(&album).unwrap(),
            vec![id("release_group", "trusted")]
        );
        assert_eq!(
            lib.musicbrainz_review(&album).unwrap().reason,
            Reason::ExistingGroupConflict
        );
        assert!(
            lib.musicbrainz_review(&album)
                .unwrap()
                .candidates
                .iter()
                .any(|c| c.metadata.identity.external_id == "alternate" && c.programs.is_some())
        );
    }
    #[test]
    fn lifecycle_labels_budget_empty_and_program_rejection_separately() {
        for (pages, expected) in [
            (
                (0..3)
                    .map(|n| Page {
                        items: vec![],
                        next_offset: Some((n + 1) * 10),
                    })
                    .collect::<Vec<_>>(),
                Reason::DiscoveryBudget,
            ),
            (vec![], Reason::NoCandidates),
            (
                vec![Page {
                    items: vec![candidate("角度", "native")],
                    next_offset: None,
                }],
                Reason::InsufficientEvidence,
            ),
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let (mut lib, album, _) = library(&tmp.path().join("db"));
            let music_library::album_matching::Preparation::Ready(i) =
                lib.prepare_album_match(&album).unwrap()
            else {
                panic!()
            };
            lib.attach_artist_external_identity(&i.artist_id, &id("artist", "band"))
                .unwrap();
            let mut p = Provider {
                pages,
                ..Default::default()
            };
            lib.reconcile_musicbrainz(&album, &mut p).unwrap();
            assert_eq!(lib.musicbrainz_review(&album).unwrap().reason, expected);
            assert!(
                lib.connection_tracks("musicbrainz", false, &album, None, 200)
                    .unwrap()
                    .iter()
                    .all(
                        |t| t.reason != "Not checked yet" && t.reason != "No MusicBrainz Recording"
                    )
            );
        }
    }
    #[test]
    fn artist_pages_are_shared_and_cache_replay_does_not_renew_provider_freshness() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db");
        let (mut lib, album, _) = library(&path);
        let music_library::album_matching::Preparation::Ready(i) =
            lib.prepare_album_match(&album).unwrap()
        else {
            panic!()
        };
        lib.attach_artist_external_identity(&i.artist_id, &id("artist", "band"))
            .unwrap();
        let mut p = Provider {
            pages: vec![Page {
                items: vec![candidate("工作", "work")],
                next_offset: None,
            }],
            programs: vec![pg("work", &["Track 1", "Track 2", "Track 3", "Track 4"])],
            ..Default::default()
        };
        lib.reconcile_musicbrainz(&album, &mut p).unwrap();
        let other = lib
            .create_catalog_release(&CatalogReleaseInput {
                title: "Other Album".into(),
                year: None,
                artists: vec![ArtistCreditInput {
                    name: "Band".into(),
                    role: None,
                }],
                tracks: vec![CatalogTrackInput {
                    title: "Other Song".into(),
                    disc_number: Some(1),
                    track_number: Some(1),
                    artists: vec![],
                }],
            })
            .unwrap();
        let other = lib.album_id_for_track(&other.track_ids[0]).unwrap();
        let music_library::album_matching::Preparation::Ready(i) =
            lib.prepare_album_match(&other).unwrap()
        else {
            panic!()
        };
        lib.attach_artist_external_identity(&i.artist_id, &id("artist", "band"))
            .unwrap();
        let cached = lib.retained_musicbrainz_discovery(&other).unwrap().unwrap();
        assert_eq!(cached.pages.len(), 1);
        assert!(cached.title_page.is_none());
        assert_eq!(cached.pages[0].page.items[0].title, "工作");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute("UPDATE musicbrainz_discovery SET observed_at=unixepoch()-86400*8,snapshot_json=json_set(snapshot_json,'$.observed_at',unixepoch()-86400*8)",[]).unwrap();
        db.execute("UPDATE musicbrainz_candidate SET observed_at=unixepoch()-86400*8,program_observed_at=unixepoch()-86400*8",[]).unwrap();
        let before:i64=db.query_row("SELECT program_observed_at FROM musicbrainz_candidate WHERE album_id=?1 AND group_id='work'",[album.as_ref()],|r|r.get(0)).unwrap();
        p.calls.clear();
        lib.reconcile_musicbrainz(&album, &mut p).unwrap();
        assert!(p.calls.is_empty());
        assert_eq!(db.query_row("SELECT program_observed_at FROM musicbrainz_candidate WHERE album_id=?1 AND group_id='work'",[album.as_ref()],|r|r.get::<_,i64>(0)).unwrap(),before);
        db.execute(
            "UPDATE musicbrainz_discovery SET observed_at=unixepoch()-86400*31",
            [],
        )
        .unwrap();
        db.execute(
            "UPDATE musicbrainz_artist_page SET observed_at=unixepoch()-86400*31",
            [],
        )
        .unwrap();
        assert!(
            lib.retained_musicbrainz_discovery(&album)
                .unwrap()
                .is_none()
        );
    }
}
