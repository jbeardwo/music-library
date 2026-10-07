use music_library::{
    Library,
    catalog::{Album, Credit, Medium, Page, Release, Track},
    domain::*,
    song_resolution::{Candidate, FinalDecision, Selection, evaluate},
    spotify_lifecycle::EVALUATION_VERSION,
};
use rusqlite::Connection;
fn id(p: &str, k: &str, v: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: p.into(),
        kind: k.into(),
        external_id: v.into(),
    }
}
fn catalog() -> Release {
    let credits = vec![Credit {
        identity: Some(id("musicbrainz", "artist", "band")),
        name: "Band".into(),
        join_phrase: String::new(),
    }];
    Release {
        album: Album {
            release_type: None,
            identity: id("musicbrainz", "release_group", "group"),
            title: "Untitled".into(),
            date: "2013-06-12".into(),
            credits: credits.clone(),
        },
        identity: id("musicbrainz", "release", "edition"),
        identities: vec![],
        title: "Untitled".into(),
        date: "2013-06-12".into(),
        credits: credits.clone(),
        media: vec![Medium {
            position: 1,
            tracks: (1..=4)
                .map(|n| Track {
                    duration: None,
                    position: n,
                    title: format!("Song {n}"),
                    credits: credits.clone(),
                    identities: vec![id("musicbrainz", "track", &format!("t{n}"))],
                })
                .collect(),
        }],
    }
}
fn candidate(input: &music_library::song_resolution::Input) -> Candidate {
    Candidate {
        identity: id(
            "spotify",
            "track",
            &format!("spotify-{}", input.number.unwrap_or(1)),
        ),
        album_identity: Some(id("spotify", "album", "album")),
        title: input.title.clone(),
        artist: "Band".into(),
        artists: input.artists.clone(),
        album: "[untitled]".into(),
        date: "2013-06-12".into(),
        album_artists: input.album_artists.clone(),
        album_type: "EP".into(),
        album_total_tracks: Some(4),
        duration_ms: 200000,
        disc: 1,
        number: input.number.unwrap_or(1),
    }
}
fn page(c: Candidate) -> Page<Candidate> {
    Page {
        items: vec![c],
        next_offset: None,
    }
}
fn fixture() -> (tempfile::TempDir, Library, ImportedRelease) {
    let dir = tempfile::tempdir().unwrap();
    let mut l = Library::open(dir.path().join("db")).unwrap();
    let imported = l.add_catalog_release(&catalog()).unwrap();
    (dir, l, imported)
}
fn db(dir: &tempfile::TempDir) -> Connection {
    Connection::open(dir.path().join("db")).unwrap()
}
fn old(dir: &tempfile::TempDir, track: &TrackId) {
    db(dir)
        .execute(
            "UPDATE spotify_connection_review SET evaluation_version=0,stale=0 WHERE track_id=?1",
            [track.as_ref()],
        )
        .unwrap();
}
fn drain(l: &mut Library) -> Vec<TrackId> {
    let mut ids = vec![];
    for _ in 0..20 {
        let b = l.reevaluate_stale_spotify(32).unwrap();
        ids.extend(b.accepted);
        if b.examined == 0 {
            return ids;
        }
    }
    panic!("current-version results must stop rerunning");
}
#[test]
fn old_cached_acceptance_resolves_locally_and_missing_cache_needs_retry_once() {
    let (dir, mut l, r) = fixture();
    let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    let p = page(candidate(&input));
    l.persist_spotify_song_review(&input, &p).unwrap();
    old(&dir, &r.track_ids[0]);
    assert!(
        l.stale_spotify_tracks(32)
            .unwrap()
            .contains(&r.track_ids[0])
    );
    let b = l.reevaluate_stale_spotify(32).unwrap();
    assert_eq!(b.accepted, vec![r.track_ids[0].clone()]);
    assert_eq!(b.needs_retry, 3);
    assert!(
        !l.track_provider_occurrences(&r.track_ids[0], "spotify")
            .unwrap()
            .is_empty()
    );
    assert_eq!(l.unresolved_spotify_count().unwrap(), 3);
    // The new trusted anchor can invalidate the remaining Album rows once.
    drain(&mut l);
    assert!(l.stale_spotify_tracks(32).unwrap().is_empty());
    drop(l);
    let mut l = Library::open(dir.path().join("db")).unwrap();
    assert_eq!(l.reevaluate_stale_spotify(32).unwrap().examined, 0);
    // Test a semantic version upgrade without changing a trusted association.
    db(&dir)
        .execute(
            "UPDATE spotify_connection_review SET evaluation_version=?1",
            [EVALUATION_VERSION - 1],
        )
        .unwrap();
    assert_eq!(l.stale_spotify_tracks(32).unwrap().len(), 3);
}
#[test]
fn current_rejection_is_not_sticky_after_relevant_evidence_changes() {
    let (dir, mut l, r) = fixture();
    let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    let mut c = candidate(&input);
    c.title = "Song 1 (Live)".into();
    l.persist_spotify_song_review(&input, &page(c)).unwrap();
    drain(&mut l);
    assert!(l.stale_spotify_tracks(32).unwrap().is_empty());
    // Persisted MusicBrainz observations invalidate this Album's unresolved program.
    db(&dir)
        .execute(
            "UPDATE track_provider_evidence SET duration_ms=200000 WHERE track_id=?1",
            [r.track_ids[0].as_ref()],
        )
        .unwrap();
    assert_eq!(l.stale_spotify_tracks(32).unwrap().len(), 4);
    let b = l.reevaluate_stale_spotify(32).unwrap();
    assert!(b.accepted.is_empty());
    assert_eq!(b.still_unresolved, 1);
    assert!(l.stale_spotify_tracks(32).unwrap().is_empty());
}
#[test]
fn evidence_and_artist_equivalence_invalidate_only_relevant_tracks() {
    let (dir, mut l, r) = fixture();
    let mut another = catalog();
    another.identity.external_id = "edition2".into();
    another.album.identity.external_id = "group2".into();
    another.album.credits[0].identity = Some(id("musicbrainz", "artist", "other"));
    another.album.credits[0].name = "Other".into();
    another.credits = another.album.credits.clone();
    for t in &mut another.media[0].tracks {
        t.credits = another.credits.clone();
    }
    let other = l.add_catalog_release(&another).unwrap();
    drain(&mut l);
    let sql = db(&dir);
    sql.execute("UPDATE album_provider_evidence SET release_type='EP' WHERE album_id=(SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id=?1)",[r.track_ids[0].as_ref()]).unwrap();
    let stale = l.stale_spotify_tracks(32).unwrap();
    assert_eq!(stale.len(), 4);
    assert!(!stale.contains(&other.track_ids[0]));
    drain(&mut l);
    let a: String = sql
        .query_row(
            "SELECT artist_id FROM track_artist_credit WHERE track_id=?1 LIMIT 1",
            [r.track_ids[0].as_ref()],
            |r| r.get(0),
        )
        .unwrap();
    sql.execute(
        "INSERT INTO artist(id,name) VALUES('rename','Renamed Band')",
        [],
    )
    .unwrap();
    sql.execute("INSERT INTO artist_equivalence(artist_a,artist_b) VALUES(min(?1,'rename'),max(?1,'rename'))",[a]).unwrap();
    let stale = l.stale_spotify_tracks(32).unwrap();
    assert_eq!(stale.len(), 4);
    assert!(!stale.contains(&other.track_ids[0]));
}
#[test]
fn exclusion_is_durable_reversible_and_preserves_user_state() {
    let (dir, mut l, r) = fixture();
    let t = &r.track_ids[0];
    let playlist = l.create_playlist("Keep").unwrap();
    l.append_playlist_track(&playlist, t).unwrap();
    let before = l.diagnostic_track_identities(t).unwrap();
    let sql = db(&dir);
    sql.execute(
        "INSERT INTO playable_source(id,kind) VALUES('test-source','local_file')",
        [],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO track_source(track_id,source_id) VALUES(?1,'test-source')",
        [t.as_ref()],
    )
    .unwrap();
    l.mark_not_on_spotify(t).unwrap();
    assert!(l.spotify_manually_excluded(t).unwrap());
    assert_eq!(l.unresolved_spotify_count().unwrap(), 3);
    assert_eq!(l.marked_spotify_count().unwrap(), 1);
    let rows = l
        .browse(&music_library::browse::Request {
            unresolved_spotify: true,
            ..Default::default()
        })
        .unwrap();
    assert!(!rows.iter().any(|row| row.id == t.as_ref()));
    let rows = l
        .browse(&music_library::browse::Request {
            unresolved_spotify: true,
            marked_spotify: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, t.as_ref());
    // Neither a new matcher version nor new canonical evidence undoes the mark.
    old(&dir, t);
    sql.execute(
        "UPDATE track_provider_evidence SET duration_ms=100000 WHERE track_id=?1",
        [t.as_ref()],
    )
    .unwrap();
    assert!(!l.stale_spotify_tracks(32).unwrap().contains(t));
    drain(&mut l);
    assert!(l.spotify_manually_excluded(t).unwrap());
    drop(l);
    let mut l = Library::open(dir.path().join("db")).unwrap();
    assert!(l.spotify_manually_excluded(t).unwrap());
    assert_eq!(l.diagnostic_track_identities(t).unwrap(), before);
    assert_eq!(l.playlist_entries(&playlist, None, 10).unwrap().len(), 1);
    assert_eq!(
        sql.query_row("SELECT count(*) FROM library_membership", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        sql.query_row(
            "SELECT count(*) FROM track_source WHERE source_id='test-source'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        1
    );
    l.check_spotify_again(t).unwrap();
    assert!(!l.spotify_manually_excluded(t).unwrap());
    assert!(l.stale_spotify_tracks(32).unwrap().contains(t));
    assert_eq!(l.marked_spotify_count().unwrap(), 0);
    assert_eq!(l.unresolved_spotify_count().unwrap(), 4);
}
#[test]
fn exclusion_blocks_automatic_acceptance_but_manual_connection_clears_it() {
    let (_dir, mut l, r) = fixture();
    let t = &r.track_ids[0];
    l.mark_not_on_spotify(t).unwrap();
    let input = l.song_resolution_input(t).unwrap();
    let c = candidate(&input);
    let p = page(c.clone());
    let e = evaluate(&input, &p, 0, &[]).unwrap();
    assert_ne!(e.final_decision, FinalDecision::Accept);
    assert_eq!(e.primary_code, Some("manually_excluded"));
    assert!(l.apply_song_evaluation(&input, &p).unwrap().is_none());
    l.confirm_song_resolution(&Selection::new(input, vec![c]), 0)
        .unwrap();
    assert!(!l.spotify_manually_excluded(t).unwrap());
    assert_eq!(l.marked_spotify_count().unwrap(), 0);
    assert!(l.mark_not_on_spotify(t).is_err());
}
#[test]
fn retry_plan_is_album_bounded_deduplicated_and_skips_exclusions_and_trust() {
    let (_dir, mut l, r) = fixture();
    assert_eq!(l.spotify_retry_albums(None, 999).unwrap().len(), 1);
    for t in &r.track_ids {
        l.mark_not_on_spotify(t).unwrap();
    }
    assert!(l.spotify_retry_albums(None, 20).unwrap().is_empty());
    let album = l.album_id_for_track(&r.track_ids[0]).unwrap();
    assert!(matches!(
        l.prepare_album_match_for(
            &album,
            &music_library::catalog::MatchingScope {
                provider: "spotify".into(),
                artist_kind: "artist".into(),
                album_kind: "album".into()
            }
        )
        .unwrap(),
        music_library::album_matching::Preparation::Done(
            music_library::album_matching::MatchOutcome::Skipped
        )
    ));
    l.check_spotify_again(&r.track_ids[0]).unwrap();
    assert_eq!(l.spotify_retry_albums(None, 20).unwrap(), vec![album]);
    let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    l.apply_song_evaluation(&input, &page(candidate(&input)))
        .unwrap();
    assert!(l.spotify_retry_albums(None, 20).unwrap().is_empty());
    old(&_dir, &r.track_ids[0]);
    assert!(
        !l.stale_spotify_tracks(32)
            .unwrap()
            .contains(&r.track_ids[0])
    );
}
#[test]
fn cache_replay_preserves_competition_and_requires_new_search_for_changed_scope() {
    let (dir, mut l, r) = fixture();
    let t = &r.track_ids[0];
    let input = l.song_resolution_input(t).unwrap();
    let c = candidate(&input);
    let mut second = c.clone();
    second.identity.external_id = "competitor".into();
    l.persist_spotify_song_review(
        &input,
        &Page {
            items: vec![c, second],
            next_offset: None,
        },
    )
    .unwrap();
    old(&dir, t);
    let b = l.reevaluate_stale_spotify(32).unwrap();
    assert!(b.accepted.is_empty());
    assert_eq!(b.still_unresolved, 1);
    l.set_track_title_override(t, "New song").unwrap();
    let b = l.reevaluate_stale_spotify(32).unwrap();
    assert!(b.accepted.is_empty());
    assert!(b.needs_retry > 0);
}
#[test]
fn stale_acceptance_failure_is_explicit_and_not_retried_in_a_loop() {
    let (dir, mut l, r) = fixture();
    let t = &r.track_ids[0];
    let input = l.song_resolution_input(t).unwrap();
    l.persist_spotify_song_review(&input, &page(candidate(&input)))
        .unwrap();
    old(&dir, t);
    db(&dir).execute_batch("CREATE TRIGGER fail_lifecycle BEFORE INSERT ON track_external_identity WHEN NEW.provider='spotify' BEGIN SELECT RAISE(ABORT,'test failure'); END").unwrap();
    let b = l.reevaluate_stale_spotify(32).unwrap();
    assert!(b.accepted.is_empty());
    assert_eq!(b.errors.len(), 1);
    assert!(b.errors[0].contains("persistence failed"));
    assert!(l.stale_spotify_tracks(32).unwrap().is_empty());
}
fn stored_program(
    l: &mut Library,
    r: &ImportedRelease,
) -> (AlbumId, music_library::album_program::Programs) {
    let album = l.album_id_for_track(&r.track_ids[0]).unwrap();
    let identity = id("spotify", "album", "program");
    l.attach_album_external_identity(&album, &identity).unwrap();
    let tracks = l
        .local_album_tracks(&album)
        .unwrap()
        .into_iter()
        .map(|t| {
            let mut e = t.evidence;
            e.identities = vec![id(
                "spotify",
                "track",
                &format!("program-{}", e.number.unwrap()),
            )];
            e.recording = Default::default();
            e
        })
        .collect();
    (
        album,
        music_library::album_program::Programs {
            album: identity,
            programs: vec![music_library::album_program::Program {
                identity: None,
                tracks,
                complete: true,
            }],
            note: "Persisted bounded program".into(),
        },
    )
}
#[test]
fn persisted_album_program_resolves_multiple_tracks_and_skips_marked_track() {
    let (dir, mut l, r) = fixture();
    let (album, p) = stored_program(&mut l, &r);
    l.mark_not_on_spotify(&r.track_ids[0]).unwrap();
    db(&dir)
        .execute(
            "INSERT INTO spotify_program_cache(album_id,programs_json) VALUES(?1,?2)",
            rusqlite::params![album.as_ref(), serde_json::to_string(&p).unwrap()],
        )
        .unwrap();
    let b = l.reevaluate_stale_spotify(32).unwrap();
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.accepted.len(), 3);
    assert!(l.spotify_manually_excluded(&r.track_ids[0]).unwrap());
    assert!(
        l.track_provider_occurrences(&r.track_ids[0], "spotify")
            .unwrap()
            .is_empty()
    );
    assert!(l.spotify_retry_albums(None, 20).unwrap().is_empty());
    assert_eq!(l.unresolved_spotify_count().unwrap(), 0);
    assert_eq!(l.marked_spotify_count().unwrap(), 1);
    // Explicit program retries still skip the mark and preserve trusted associations.
    let input = l.prepare_album_program(&album, &p.album).unwrap().unwrap();
    l.complete_album_program(music_library::album_program::Reply {
        input,
        result: Ok(p),
    })
    .unwrap();
    assert_eq!(l.marked_spotify_count().unwrap(), 1);
}
#[test]
fn rejected_cached_song_does_not_suppress_current_cached_album_program() {
    let (dir, mut l, r) = fixture();
    let (album, program) = stored_program(&mut l, &r);
    db(&dir)
        .execute(
            "INSERT INTO spotify_program_cache(album_id,programs_json) VALUES(?1,?2)",
            rusqlite::params![album.as_ref(), serde_json::to_string(&program).unwrap()],
        )
        .unwrap();
    for track in &r.track_ids {
        let input = l.song_resolution_input(track).unwrap();
        let mut wrong = candidate(&input);
        wrong.disc = 2;
        l.persist_spotify_song_review(&input, &page(wrong)).unwrap();
        old(&dir, track);
    }
    let batch = l.reevaluate_stale_spotify(32).unwrap();
    assert!(batch.errors.is_empty(), "{:?}", batch.errors);
    assert_eq!(batch.accepted.len(), 4);
    assert_eq!(batch.still_unresolved, 0);
    assert_eq!(batch.needs_retry, 0);
    assert_eq!(l.unresolved_spotify_count().unwrap(), 0);
    assert!(l.stale_spotify_tracks(32).unwrap().is_empty());
}
#[test]
fn retry_track_cursor_reaches_every_album_without_repeating_failed_tracks() {
    let (_dir, mut l, r) = fixture();
    let album = l.album_id_for_track(&r.track_ids[0]).unwrap();
    let mut after = None;
    let mut seen = std::collections::HashSet::new();
    let mut previous_position = 0;
    loop {
        let page = l
            .spotify_retry_tracks_after(std::slice::from_ref(&album), after.as_ref(), 1)
            .unwrap();
        assert!(page.len() <= 1);
        if page.is_empty() {
            break;
        }
        after = page.last().cloned();
        assert!(seen.insert(page[0].clone()));
        // A failed lookup remains unresolved, but the cursor must still advance.
        let input = l.song_resolution_input(&page[0]).unwrap();
        assert!(input.number.unwrap() > previous_position);
        previous_position = input.number.unwrap();
        l.persist_spotify_song_review(
            &input,
            &Page {
                items: vec![],
                next_offset: None,
            },
        )
        .unwrap();
    }
    assert_eq!(seen.len(), r.track_ids.len());
}
#[test]
fn retry_album_cursor_follows_alphabetical_titles_with_stable_id_ties() {
    use music_library::domain::{ArtistCreditInput, CatalogReleaseInput, CatalogTrackInput};
    let dir = tempfile::tempdir().unwrap();
    let mut l = Library::open(dir.path().join("db")).unwrap();
    let mut expected = vec![];
    for title in ["Z last", "A first", "M middle", "A first"] {
        let artist = ArtistCreditInput {
            name: "Band".into(),
            role: None,
        };
        let r = l
            .create_catalog_release(&CatalogReleaseInput {
                title: title.into(),
                year: None,
                artists: vec![artist.clone()],
                tracks: vec![CatalogTrackInput {
                    title: "Song".into(),
                    artists: vec![artist],
                    disc_number: Some(1),
                    track_number: Some(1),
                }],
            })
            .unwrap();
        l.add_to_library(&r.track_ids[0]).unwrap();
        expected.push((
            title.to_lowercase(),
            l.album_id_for_track(&r.track_ids[0]).unwrap(),
        ));
    }
    expected.sort_by(|a, b| (&a.0, a.1.as_ref()).cmp(&(&b.0, b.1.as_ref())));
    let mut after = None;
    let mut actual = vec![];
    loop {
        let page = l.spotify_retry_albums(after.as_ref(), 1).unwrap();
        if page.is_empty() {
            break;
        }
        after = page.last().cloned();
        actual.extend(page);
    }
    assert_eq!(
        actual,
        expected.into_iter().map(|(_, id)| id).collect::<Vec<_>>()
    );
}
#[test]
fn spotify_duration_fadeout_tolerance_keeps_other_identity_gates() {
    for delta in [4_000, 10_000, 10_001] {
        let dir = tempfile::tempdir().unwrap();
        let mut l = Library::open(dir.path().join("db")).unwrap();
        let mut release = catalog();
        release.media[0].tracks[0].duration = Some(music_library::catalog::Duration {
            milliseconds: 200_000,
            approximate: false,
        });
        let r = l.add_catalog_release(&release).unwrap();
        let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
        let mut c = candidate(&input);
        c.duration_ms += delta;
        let p = page(c.clone());
        let evaluation = evaluate(&input, &p, 0, &[]).unwrap();
        assert_eq!(
            evaluation.final_decision == FinalDecision::Accept,
            delta <= 10_000
        );
        assert!(
            evaluation
                .fields
                .iter()
                .any(|f| f.label == "Duration" && f.evidence.contains("10 seconds"))
        );
        let mut live = c.clone();
        live.title.push_str(" (Live)");
        assert_ne!(
            evaluate(&input, &page(live), 0, &[])
                .unwrap()
                .final_decision,
            FinalDecision::Accept
        );
        let mut wrong_position = c;
        wrong_position.number = 2;
        assert_ne!(
            evaluate(&input, &page(wrong_position), 0, &[])
                .unwrap()
                .final_decision,
            FinalDecision::Accept
        );
        assert_eq!(
            l.apply_song_evaluation(&input, &p).unwrap().is_some(),
            delta <= 10_000
        );
        assert_eq!(
            !l.track_provider_occurrences(&r.track_ids[0], "spotify")
                .unwrap()
                .is_empty(),
            delta <= 10_000
        );
    }
}
#[test]
fn bulk_exclusion_is_atomic_durable_and_skips_new_connections() {
    let (dir, mut l, r) = fixture();
    l.attach_track_external_identity(&r.track_ids[3], &id("spotify", "track", "connected"))
        .unwrap();
    let mut selected = r.track_ids.clone();
    selected.push(r.track_ids[0].clone());
    assert_eq!(l.mark_tracks_not_on_spotify(&selected).unwrap(), 3);
    assert_eq!(l.marked_spotify_count().unwrap(), 3);
    assert_eq!(l.unresolved_spotify_count().unwrap(), 0);
    assert_eq!(l.mark_tracks_not_on_spotify(&selected).unwrap(), 0);
    drop(l);
    let l = Library::open(dir.path().join("db")).unwrap();
    for t in &r.track_ids[..3] {
        assert!(l.spotify_manually_excluded(t).unwrap());
    }
    assert!(!l.spotify_manually_excluded(&r.track_ids[3]).unwrap());
    assert_eq!(l.library_queue(&Default::default()).unwrap().len(), 4);
}
#[test]
fn cached_program_cannot_delete_successful_trusted_association_after_new_conflict() {
    let (dir, mut l, r) = fixture();
    let (album, p) = stored_program(&mut l, &r);
    let input = l.prepare_album_program(&album, &p.album).unwrap().unwrap();
    l.complete_album_program(music_library::album_program::Reply {
        input,
        result: Ok(p.clone()),
    })
    .unwrap();
    let trusted = l
        .track_provider_occurrences(&r.track_ids[0], "spotify")
        .unwrap();
    assert!(!trusted.is_empty());
    l.set_track_title_override(&r.track_ids[0], "Other (Live)")
        .unwrap();
    old(&dir, &r.track_ids[0]);
    assert!(
        !l.stale_spotify_tracks(32)
            .unwrap()
            .contains(&r.track_ids[0])
    );
    let input = l.prepare_album_program(&album, &p.album).unwrap().unwrap();
    l.complete_album_program(music_library::album_program::Reply {
        input,
        result: Ok(p),
    })
    .unwrap();
    assert_eq!(
        l.track_provider_occurrences(&r.track_ids[0], "spotify")
            .unwrap(),
        trusted
    );
}
#[test]
fn retry_album_plan_uses_one_search_context_for_multiple_unresolved_tracks() {
    use music_library::{album_matching::AlbumMatcher, catalog::*};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    struct Provider(Arc<AtomicUsize>);
    impl CatalogProvider for Provider {
        fn search_artists(&mut self, name: &str) -> Result<Page<ArtistCandidate>, CatalogError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Page {
                items: vec![ArtistCandidate {
                    identity: id("spotify", "artist", "band"),
                    name: name.into(),
                    aliases: vec![],
                    comment: String::new(),
                    country: String::new(),
                    artist_type: String::new(),
                    score: None,
                }],
                next_offset: None,
            })
        }
        fn artist_albums(
            &mut self,
            _: &ExternalIdentity,
            _: &str,
        ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Page {
                items: vec![],
                next_offset: None,
            })
        }
        fn search_albums(&mut self, _: &str, _: u32) -> Result<Page<AlbumCandidate>, CatalogError> {
            panic!("no broad search")
        }
        fn releases(
            &mut self,
            _: &ExternalIdentity,
            _: u32,
        ) -> Result<Page<ReleaseCandidate>, CatalogError> {
            panic!("no edition search")
        }
        fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
            panic!("no edition fetch")
        }
    }
    let (_dir, mut l, r) = fixture();
    let albums = l.spotify_retry_albums(None, 20).unwrap();
    assert_eq!(albums.len(), 1);
    let calls = Arc::new(AtomicUsize::new(0));
    let (send, recv) = mpsc::channel();
    let scope = MatchingScope {
        provider: "spotify".into(),
        artist_kind: "artist".into(),
        album_kind: "album".into(),
    };
    let mut matcher = AlbumMatcher::for_provider(
        Provider(calls.clone()),
        scope,
        move |r| send.send(r).unwrap(),
        |_| panic!("no Album program"),
        |_| {},
    )
    .unwrap();
    for _ in &r.track_ids {
        matcher.match_album(&l, &albums[0]).unwrap();
    }
    let reply = recv
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    matcher.complete(&mut l, reply);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "one Artist + one Album search, not one per Track"
    );
    for t in &r.track_ids {
        l.mark_not_on_spotify(t).unwrap();
    }
    assert!(l.spotify_retry_albums(None, 20).unwrap().is_empty());
    matcher.match_album(&l, &albums[0]).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
#[test]
fn identical_provider_observations_do_not_stale_current_failures() {
    let (dir, mut l, r) = fixture();
    drain(&mut l);
    let c = db(&dir);
    c.execute(
        "UPDATE track_provider_evidence SET title=title WHERE track_id=?1",
        [r.track_ids[0].as_ref()],
    )
    .unwrap();
    c.execute("UPDATE album_provider_evidence SET title=title", [])
        .unwrap();
    assert!(l.stale_spotify_tracks(32).unwrap().is_empty());
}

#[test]
fn discovery_ttl_restart_and_cooldown_use_controlled_time() {
    use music_library::spotify_lifecycle::DISCOVERY_TTL_SECONDS;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let l = Library::open(&path).unwrap();
    let now = 1_800_000_000;
    l.persist_spotify_discovery("search:v1:album", "{\"items\":[]}", now)
        .unwrap();
    l.retain_spotify_cooldown(now + 5893).unwrap();
    drop(l);
    let l = Library::open(&path).unwrap();
    assert!(
        l.spotify_discovery_response("search:v1:album", now + DISCOVERY_TTL_SECONDS - 1)
            .unwrap()
            .is_some()
    );
    assert!(
        l.spotify_discovery_response("search:v1:album", now + DISCOVERY_TTL_SECONDS)
            .unwrap()
            .is_none()
    );
    assert!(
        l.spotify_discovery_response("search:v2:album", now)
            .unwrap()
            .is_none()
    );
    assert_eq!(l.spotify_cooldown_remaining(now).unwrap(), 5893);
    assert_eq!(l.spotify_cooldown_remaining(now + 5892).unwrap(), 1);
    assert_eq!(l.spotify_cooldown_remaining(now + 5893).unwrap(), 0);
    l.retain_spotify_cooldown(now + 1).unwrap();
    assert_eq!(l.spotify_cooldown_remaining(now).unwrap(), 5893);
}

#[test]
fn empty_completed_track_search_replays_after_evaluator_version_change() {
    let (dir, mut library, imported) = fixture();
    drain(&mut library);
    let input = library
        .song_resolution_input(&imported.track_ids[0])
        .unwrap();
    library
        .persist_spotify_song_review(
            &input,
            &Page {
                items: vec![],
                next_offset: None,
            },
        )
        .unwrap();
    old(&dir, &input.track_id);
    let batch = library.reevaluate_stale_spotify(32).unwrap();
    assert_eq!(batch.needs_retry, 0);
    assert_eq!(batch.still_unresolved, 1);
    assert_eq!(batch.errors.len(), 0);
    assert!(library.stale_spotify_tracks(32).unwrap().is_empty());
}

#[test]
fn retry_after_delta_and_http_date_use_the_actual_deadline() {
    use music_library::spotify_lifecycle::spotify_retry_delay;
    let now = 1_800_000_000;
    assert_eq!(spotify_retry_delay(Some("5893"), now), 5893);
    let date = httpdate::fmt_http_date(
        std::time::UNIX_EPOCH + std::time::Duration::from_secs((now + 5893) as u64),
    );
    assert_eq!(spotify_retry_delay(Some(&date), now), 5893);
    assert_eq!(spotify_retry_delay(Some("invalid"), now), 30);
}

#[test]
fn three_persisted_candidates_survive_old_rejection_and_new_evaluator_acceptance() {
    let (dir, mut library, imported) = fixture();
    drain(&mut library);
    let input = library
        .song_resolution_input(&imported.track_ids[0])
        .unwrap();
    let good = candidate(&input);
    let mut live = good.clone();
    live.identity.external_id = "other-live".into();
    live.title.push_str(" (Live)");
    let mut remix = good.clone();
    remix.identity.external_id = "other-remix".into();
    remix.title.push_str(" (Remix)");
    library
        .persist_spotify_song_review(
            &input,
            &Page {
                items: vec![live, good, remix],
                next_offset: None,
            },
        )
        .unwrap();
    // Historical evaluator decision is independent of the successful candidate page.
    db(&dir).execute("UPDATE spotify_connection_review SET evaluation_version=0,state='unresolved',stale=0 WHERE track_id=?1",[input.track_id.as_ref()]).unwrap();
    drop(library);
    let mut library = Library::open(dir.path().join("db")).unwrap();
    let batch = library.reevaluate_stale_spotify(32).unwrap();
    assert_eq!(batch.accepted, vec![input.track_id.clone()]);
    assert_eq!(db(&dir).query_row("SELECT json_array_length(page_json,'$.items') FROM spotify_reconciliation_cache WHERE track_id=?1",[input.track_id.as_ref()],|r|r.get::<_,i64>(0)).unwrap(),3);
    assert!(
        !library
            .track_provider_occurrences(&input.track_id, "spotify")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn genre_source_availability_and_path_do_not_change_discovery_inputs() {
    let (dir, mut library, imported) = fixture();
    let track = &imported.track_ids[0];
    let sql = db(&dir);
    sql.execute_batch("INSERT INTO playable_source(id,kind) VALUES('cache-source','local_file'); INSERT INTO local_file_observation(source_id,path,size_bytes,modified_ns,available) VALUES('cache-source',X'2f6f6c642e666c6163',1,1,1);").unwrap();
    sql.execute(
        "INSERT INTO track_source(track_id,source_id) VALUES(?1,'cache-source')",
        [track.as_ref()],
    )
    .unwrap();
    drain(&mut library);
    let input = library.song_resolution_input(track).unwrap();
    library
        .persist_spotify_song_review(
            &input,
            &Page {
                items: vec![],
                next_offset: None,
            },
        )
        .unwrap();
    library
        .save_metadata(
            &music_library::metadata::Target::Track(track.as_ref().into()),
            &[music_library::metadata::Change {
                field: "genre".into(),
                value: Some("Jazz".into()),
            }],
            &[],
        )
        .unwrap();
    assert_eq!(library.song_resolution_input(track).unwrap(), input);
    sql.execute(
        "UPDATE local_file_observation SET available=0 WHERE source_id='cache-source'",
        [],
    )
    .unwrap();
    assert_eq!(library.song_resolution_input(track).unwrap(), input);
    sql.execute("UPDATE local_file_observation SET path=X'2f6e65772e666c6163' WHERE source_id='cache-source'",[]).unwrap();
    assert_eq!(library.song_resolution_input(track).unwrap(), input);
    old(&dir, track);
    let batch = library.reevaluate_stale_spotify(32).unwrap();
    assert_eq!(batch.needs_retry, 0);
    assert_eq!(batch.still_unresolved, 1);
}
