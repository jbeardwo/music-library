use music_library::{
    Library,
    canonical_evidence::Origin,
    catalog::{Album, Credit, Medium, Page, Release, Track},
    domain::*,
    song_resolution::{Candidate, FinalDecision, evaluate},
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
#[test]
fn catalog_only_acceptance_persists_all_tracks_without_local_sources_or_membership_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db");
    let mut l = Library::open(&path).unwrap();
    let r = l.add_catalog_release(&catalog()).unwrap();
    for t in &r.track_ids {
        let input = l.song_resolution_input(t).unwrap();
        assert!(input.evidence.titles.iter().any(
            |o| matches!(&o.origin,Origin::Provider{identity} if identity.provider=="musicbrainz")
        ));
        let p = page(candidate(&input));
        assert_eq!(
            evaluate(&input, &p, 0, &[]).unwrap().final_decision,
            FinalDecision::Accept
        );
        assert!(l.apply_song_evaluation(&input, &p).unwrap().is_some());
        assert!(
            l.list_track_external_identities(t)
                .unwrap()
                .iter()
                .any(|id| id.provider == "spotify")
        );
    }
    let playlist = l.create_playlist("Catalog-only").unwrap();
    l.append_playlist_track(&playlist, &r.track_ids[0]).unwrap();
    let entries = l.playlist_entries(&playlist, None, 10).unwrap();
    assert_eq!(entries[0].track.as_ref().unwrap().track_id, r.track_ids[0]);
    assert!(
        l.list_track_external_identities(&entries[0].track.as_ref().unwrap().track_id)
            .unwrap()
            .iter()
            .any(|id| id.provider == "spotify")
    );
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM library_membership", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM track_source", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
    assert_eq!(db.query_row("SELECT count(*) FROM library_membership m JOIN trusted_spotify_track s ON s.track_id=m.track_id",[],|r|r.get::<_,u32>(0)).unwrap(),4);
    drop(l);
    let l = Library::open(&path).unwrap();
    assert_eq!(
        l.local_album_tracks(&AlbumId(
            db.query_row(
                "SELECT album_id FROM release WHERE id=?1",
                [r.release_id.as_ref()],
                |r| r.get(0)
            )
            .unwrap()
        ))
        .unwrap()
        .len(),
        4
    );
}
#[test]
fn missing_fields_are_neutral_and_provider_metadata_supplies_evidence() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db");
    let mut l = Library::open(&path).unwrap();
    let r = l.add_catalog_release(&catalog()).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute("UPDATE album_application_metadata SET year=NULL", [])
        .unwrap();
    db.execute("UPDATE track SET disc_number=NULL", []).unwrap();
    let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    assert_eq!(input.album_date.unwrap().year, 2013);
    let p = page(candidate(&input));
    let e = evaluate(&input, &p, 0, &[]).unwrap();
    assert_eq!(e.final_decision, FinalDecision::Accept);
    assert_eq!(
        e.fields
            .iter()
            .find(|f| f.label == "Release date")
            .unwrap()
            .status,
        "equivalent"
    );
    assert_eq!(
        e.fields.iter().find(|f| f.label == "Disc").unwrap().status,
        "equivalent"
    );
    assert_eq!(
        e.fields
            .iter()
            .find(|f| f.label == "Release type")
            .unwrap()
            .status,
        "unknown"
    );
    assert!(!e.warnings.iter().any(|w| w.contains("unknown")));
    db.execute("UPDATE album_provider_evidence SET release_type='EP'", [])
        .unwrap();
    let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    assert_eq!(
        evaluate(&input, &p, 0, &[])
            .unwrap()
            .fields
            .iter()
            .find(|f| f.label == "Release type")
            .unwrap()
            .status,
        "equivalent"
    );
    db.execute("UPDATE track SET disc_number=2", []).unwrap();
    let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    assert_eq!(
        evaluate(&input, &p, 0, &[]).unwrap().primary_code,
        Some("position_mismatch")
    );
}
#[test]
fn missing_all_known_dates_is_neutral() {
    let mut l = Library::open_in_memory().unwrap();
    let mut c = catalog();
    c.date.clear();
    c.album.date.clear();
    let r = l.add_catalog_release(&c).unwrap();
    let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    let p = page(candidate(&input));
    let e = evaluate(&input, &p, 0, &[]).unwrap();
    assert_eq!(
        e.fields
            .iter()
            .find(|f| f.label == "Release date")
            .unwrap()
            .status,
        "unknown"
    );
    assert_eq!(e.final_decision, FinalDecision::Accept);
}
#[test]
fn accepted_persistence_failure_is_distinct_and_atomic() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db");
    let mut l = Library::open(&path).unwrap();
    let r = l.add_catalog_release(&catalog()).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER fail_spotify BEFORE INSERT ON track_external_identity WHEN NEW.provider='spotify' BEGIN SELECT RAISE(ABORT,'injected database failure'); END;").unwrap();
    let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    let p = page(candidate(&input));
    assert_eq!(
        evaluate(&input, &p, 0, &[]).unwrap().final_decision,
        FinalDecision::Accept
    );
    let error = l.apply_song_evaluation(&input, &p).unwrap_err().to_string();
    assert!(error.contains("accepted but association persistence failed"));
    assert!(error.contains("injected database failure"));
    assert!(
        !l.list_track_external_identities(&r.track_ids[0])
            .unwrap()
            .iter()
            .any(|id| id.provider == "spotify")
    );
}
#[test]
fn existing_provider_guard_and_stale_evidence_are_visible() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db");
    let mut l = Library::open(&path).unwrap();
    let r = l.add_catalog_release(&catalog()).unwrap();
    let old = l.song_resolution_input(&r.track_ids[0]).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "INSERT INTO track_external_identity VALUES(?1,'spotify','track','other')",
        [r.track_ids[0].as_ref()],
    )
    .unwrap();
    let current = l.song_resolution_input(&r.track_ids[0]).unwrap();
    let p = page(candidate(&current));
    let trusted = l.list_track_external_identities(&r.track_ids[0]).unwrap();
    assert_eq!(
        evaluate(&current, &p, 0, &trusted).unwrap().primary_code,
        Some("trusted_identity_conflict")
    );
    assert!(l.apply_song_evaluation(&current, &p).unwrap().is_none());
    assert!(
        l.apply_song_evaluation(&old, &p)
            .unwrap_err()
            .to_string()
            .contains("Canonical evidence changed")
    );
}
#[test]
fn conflicting_observations_are_preserved_and_catalog_title_can_corroborate() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db");
    let mut l = Library::open(&path).unwrap();
    let r = l.add_catalog_release(&catalog()).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE effective_track_metadata SET title='Wrong title' WHERE track_id=?1",
        [r.track_ids[0].as_ref()],
    )
    .unwrap();
    db.execute(
        "UPDATE track_application_metadata SET title='Wrong title' WHERE track_id=?1",
        [r.track_ids[0].as_ref()],
    )
    .unwrap();
    let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    let mut c = candidate(&input);
    c.title = "Song 1".into();
    let e = evaluate(&input, &page(c), 0, &[]).unwrap();
    assert_eq!(e.final_decision, FinalDecision::Accept);
    assert!(e.provenance.contains("Wrong title (application)"));
    assert!(
        e.provenance
            .contains("Song 1 (musicbrainz release edition)")
    );
    assert!(
        e.warnings
            .iter()
            .any(|w| w.contains("observations disagree"))
    );
}
#[test]
fn presentational_normalization_preserves_internal_and_version_content() {
    use music_library::matching::*;
    assert_eq!(
        normalize_album_title("[untitled]"),
        normalize_album_title("Untitled")
    );
    assert_eq!(
        normalize_album_title("X’ed Out"),
        normalize_album_title("X'ed Out")
    );
    assert_ne!(
        normalize_album_title("Song [Live]"),
        normalize_album_title("Song")
    );
    assert_ne!(
        normalize_album_title("Song [Part One]"),
        normalize_album_title("Song Part One")
    );
    assert_eq!(presentation_title("[[title]]"), "[[title]]");
    assert_eq!(
        artist_album_title("Tera Melos on Audiotree Live", "Tera Melos"),
        Some("Audiotree Live")
    );
    assert_eq!(
        artist_album_title("Unknown on Audiotree Live", "Tera Melos"),
        None
    );
}

#[test]
fn invalid_occurrence_and_stale_search_are_part_of_final_decision() {
    use music_library::song_resolution::evaluate_attempt;
    let mut l = Library::open_in_memory().unwrap();
    let r = l.add_catalog_release(&catalog()).unwrap();
    let old = l.song_resolution_input(&r.track_ids[0]).unwrap();
    let mut c = candidate(&old);
    c.identity.kind = "recording".into();
    let p = page(c);
    let e = evaluate(&old, &p, 0, &[]).unwrap();
    assert_eq!(e.final_decision, FinalDecision::Reject);
    assert_eq!(e.primary_code, Some("invalid_candidate_identity"));
    assert!(!e.decision.starts_with("Accept"));
    assert!(l.apply_song_evaluation(&old, &p).unwrap().is_none());
    let p = page(candidate(&old));
    let mut current = old.clone();
    current.album_date = None;
    let e = evaluate_attempt(&old, &current, &p, 0, &[]).unwrap();
    assert_eq!(e.final_decision, FinalDecision::NeedsReview);
    assert_eq!(e.primary_code, Some("evidence_changed"));
}
#[test]
fn silent_database_ignore_is_reported_as_persistence_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db");
    let mut l = Library::open(&path).unwrap();
    let r = l.add_catalog_release(&catalog()).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER ignore_spotify BEFORE INSERT ON track_external_identity WHEN NEW.provider='spotify' BEGIN SELECT RAISE(IGNORE); END;").unwrap();
    let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    let p = page(candidate(&input));
    assert_eq!(
        evaluate(&input, &p, 0, &[]).unwrap().final_decision,
        FinalDecision::Accept
    );
    assert!(
        l.apply_song_evaluation(&input, &p)
            .unwrap_err()
            .to_string()
            .contains("accepted but association persistence failed")
    );
}
#[test]
fn withdrawn_provider_identity_does_not_leave_active_trusted_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db");
    let mut l = Library::open(&path).unwrap();
    let r = l.add_catalog_release(&catalog()).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "DELETE FROM release_external_identity WHERE release_id=?1",
        [r.release_id.as_ref()],
    )
    .unwrap();
    let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    assert!(
        input
            .evidence
            .titles
            .iter()
            .all(|o| !matches!(o.origin, Origin::Provider { .. }))
    );
}
