use music_library::{
    Library,
    catalog::Page,
    domain::*,
    edition::ArtistEvidence,
    song_resolution::{Assessment, Candidate, assess, evaluate},
};
fn id(provider: &str, kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: provider.into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
fn fixture(l: &mut Library) -> ImportedRelease {
    l.create_catalog_release(&CatalogReleaseInput {
        title: "First EP".into(),
        year: Some(2009),
        artists: vec![ArtistCreditInput {
            name: "Old band name".into(),
            role: None,
        }],
        tracks: (1..=4)
            .map(|n| CatalogTrackInput {
                title: format!("Song {n}"),
                disc_number: Some(1),
                track_number: Some(n),
                artists: vec![],
            })
            .collect(),
    })
    .unwrap()
}
fn candidate() -> Candidate {
    let a = ArtistEvidence {
        name: "New band name".into(),
        identities: vec![id("spotify", "artist", "new-artist")],
        join_phrase: String::new(),
    };
    Candidate {
        album_identity: None,
        identity: id("spotify", "track", "track1"),
        title: "Song 1".into(),
        artist: a.name.clone(),
        artists: vec![a.clone()],
        album: "First EP".into(),
        date: "2009".into(),
        album_artists: vec![a],
        album_type: "single".into(),
        album_total_tracks: Some(4),
        duration_ms: 222000,
        disc: 1,
        number: 1,
    }
}
fn page(c: Candidate) -> Page<Candidate> {
    Page {
        items: vec![c],
        next_offset: None,
    }
}
#[test]
fn confirmed_relationship_persists_without_merging_credits_or_provider_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.sqlite");
    let mut l = Library::open(&path).unwrap();
    let r = fixture(&mut l);
    let t = &r.track_ids[0];
    l.add_to_library(t).unwrap();
    let input = l.song_resolution_input(t).unwrap();
    let p = page(candidate());
    assert_eq!(assess(&input, &p), Assessment::NoMatch);
    let proposal = l
        .prepare_artist_equivalence(t, &p.items[0].artists[0])
        .unwrap();
    l.attach_artist_external_identity(&proposal.local, &id("spotify", "artist", "old-artist"))
        .unwrap();
    let proposal = l
        .prepare_artist_equivalence(t, &p.items[0].artists[0])
        .unwrap();
    let before = l.list_artist_external_identities(&proposal.local).unwrap();
    // Preparing/canceling never creates equivalence.
    assert_eq!(
        assess(&l.song_resolution_input(t).unwrap(), &p),
        Assessment::NoMatch
    );
    let peer = l.confirm_artist_equivalence(&proposal).unwrap();
    assert_ne!(peer, proposal.local);
    assert_eq!(
        l.list_artist_external_identities(&proposal.local).unwrap(),
        before
    );
    assert_eq!(
        l.list_artist_external_identities(&peer).unwrap(),
        vec![id("spotify", "artist", "new-artist")]
    );
    let after = l.song_resolution_input(t).unwrap();
    assert_eq!(after.artist, input.artist);
    assert_eq!(after.album, input.album);
    assert_eq!(assess(&after, &p), Assessment::Unique(0));
    assert!(l.list_track_external_identities(t).unwrap().is_empty());
    assert_eq!(l.unresolved_spotify_count().unwrap(), 1);
    let mut wrong = p.clone();
    wrong.items[0].album = "First EP (Live)".into();
    assert_ne!(assess(&after, &wrong), Assessment::Unique(0));
    wrong = p.clone();
    wrong.items[0].number = 2;
    assert_ne!(assess(&after, &wrong), Assessment::Unique(0));
    wrong = p.clone();
    wrong.items[0].title = "Song 1 (Live)".into();
    assert_ne!(assess(&after, &wrong), Assessment::Unique(0));
    drop(l);
    let l = Library::open(path).unwrap();
    assert_eq!(
        assess(&l.song_resolution_input(t).unwrap(), &p),
        Assessment::Unique(0)
    );
    assert_eq!(l.song_resolution_input(t).unwrap().artist, "Old band name");
}
#[test]
fn diagnostics_explain_real_acceptance_gates_in_identical_field_order() {
    let mut l = Library::open_in_memory().unwrap();
    let r = fixture(&mut l);
    let mut input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    let c = candidate();
    let mut p = page(c.clone());
    let e = evaluate(&input, &p, 0, &[]).unwrap();
    assert_eq!(e.primary_code, Some("artist_mismatch"));
    assert!(
        e.fields
            .iter()
            .any(|f| f.label == "Artist credit" && f.status == "conflict")
    );
    let proposal = l
        .prepare_artist_equivalence(&input.track_id, &c.artists[0])
        .unwrap();
    l.confirm_artist_equivalence(&proposal).unwrap();
    input = l.song_resolution_input(&input.track_id).unwrap();
    input.duration_ms = Some(222000);
    let e = evaluate(&input, &p, 0, &[]).unwrap();
    assert!(e.primary_blocker.is_none());
    assert_eq!(
        e.fields.iter().map(|f| f.label).collect::<Vec<_>>(),
        vec![
            "Song",
            "Artist credit",
            "Album",
            "Release type",
            "Disc",
            "Track",
            "Track count",
            "Duration",
            "Release date",
            "Trusted external IDs",
            "Artist IDs"
        ]
    );
    assert!(
        e.fields
            .iter()
            .any(|f| f.label == "Song" && f.status == "equivalent")
    );
    p.items[0].title = "SONG 1".into();
    let e = evaluate(&input, &p, 0, &[]).unwrap();
    assert!(e.primary_blocker.is_none());
    assert_eq!(e.fields[0].status, "normalized");
    p.items[0].duration_ms += 4000;
    let e = evaluate(&input, &p, 0, &[]).unwrap();
    assert_eq!(e.primary_code, Some("duration_threshold"));
    assert!(!e.warnings.is_empty());
    assert!(e.primary_blocker.unwrap().contains("3-second"));
    p = page(c.clone());
    p.next_offset = Some(10);
    assert_eq!(
        evaluate(&input, &p, 0, &[]).unwrap().primary_code,
        Some("incomplete_candidate_page")
    );
    p = page(c.clone());
    let mut other = c.clone();
    other.identity.external_id = "track2".into();
    p.items.push(other);
    assert_eq!(
        evaluate(&input, &p, 0, &[]).unwrap().primary_code,
        Some("competing_candidates")
    );
    p = page(c);
    assert_eq!(
        evaluate(&input, &p, 0, &[id("spotify", "track", "different")])
            .unwrap()
            .primary_code,
        Some("trusted_identity_conflict")
    );
    p.items[0].album_total_tracks = Some(2);
    assert_eq!(
        evaluate(&input, &p, 0, &[]).unwrap().primary_code,
        Some("track_count_mismatch")
    );
}
#[test]
fn no_automatic_alias_from_similar_names_titles_or_single_result() {
    let mut l = Library::open_in_memory().unwrap();
    let r = fixture(&mut l);
    let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    let mut c = candidate();
    for name in [
        "Old band names",
        "Old band name live",
        "Old-band-name?",
        "New band name",
    ] {
        c.artist = name.into();
        c.artists[0].name = name.into();
        assert_ne!(assess(&input, &page(c.clone())), Assessment::Unique(0));
    }
    let p = l
        .prepare_artist_equivalence(&input.track_id, &c.artists[0])
        .unwrap();
    assert!(p.local_identities.is_empty());
    assert_eq!(l.song_resolution_input(&input.track_id).unwrap(), input);
}

#[test]
fn album_typography_is_normalized_without_accepting_semantic_versions() {
    let mut l = Library::open_in_memory().unwrap();
    let r = fixture(&mut l);
    let mut input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    let mut c = candidate();
    c.artist = input.artist.clone();
    c.artists = input.artists.clone();
    c.album_artists = input.album_artists.clone();
    input.album = "There’s No 666 in Outer Space".into();
    c.album = "There's No 666 in Outer Space".into();
    assert_eq!(assess(&input, &page(c.clone())), Assessment::Unique(0));
    let e = evaluate(&input, &page(c.clone()), 0, &[]).unwrap();
    assert_eq!(e.fields[2].status, "normalized");
    c.album.push_str(" (Live)");
    assert_ne!(assess(&input, &page(c)), Assessment::Unique(0));
}

#[test]
fn stale_confirmation_is_rejected_and_links_survive_existing_identity_consolidation() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let r = fixture(&mut l);
    let c = candidate();
    let stale = l
        .prepare_artist_equivalence(&r.track_ids[0], &c.artists[0])
        .unwrap();
    l.attach_artist_external_identity(&stale.local, &id("musicbrainz", "artist", "old-credit-id"))
        .unwrap();
    assert!(l.confirm_artist_equivalence(&stale).is_err());
    let proposal = l
        .prepare_artist_equivalence(&r.track_ids[0], &c.artists[0])
        .unwrap();
    let peer = l.confirm_artist_equivalence(&proposal).unwrap();
    let canonical = ArtistId(uuid::Uuid::new_v4().to_string());
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "INSERT INTO artist(id,name) VALUES(?1,'Consolidated identity')",
        [canonical.as_ref()],
    )
    .unwrap();
    l.merge_artist(&proposal.local, &canonical).unwrap();
    let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    assert_eq!(input.artist, "Old band name");
    assert_eq!(assess(&input, &page(c)), Assessment::Unique(0));
    assert_eq!(
        l.list_artist_external_identities(&peer).unwrap(),
        vec![id("spotify", "artist", "new-artist")]
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM artist_equivalence WHERE artist_a=?1 OR artist_b=?1",
            [canonical.as_ref()],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn equivalence_migration_failure_rolls_back_version() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    drop(Library::open(&path).unwrap());
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("DROP TABLE artist_equivalence; CREATE TABLE artist_equivalence(sentinel TEXT); PRAGMA user_version=26;").unwrap();
    assert!(Library::open(&path).is_err());
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        26
    );
}
