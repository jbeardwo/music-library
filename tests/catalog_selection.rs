use music_library::{catalog::*, domain::ExternalIdentity};
use std::sync::{Arc, Mutex};
fn id(kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "test".into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
fn album() -> AlbumCandidate {
    AlbumCandidate {
        identity: id("album", "a"),
        title: "Friendly".into(),
        artist: "Artist".into(),
        credits: vec![],
        date: "2005-05-11".into(),
        primary_type: "Album".into(),
        secondary_types: vec![],
        comment: String::new(),
        score: None,
    }
}
fn edition(key: &str, date: &str, comment: &str, discs: usize) -> ReleaseCandidate {
    ReleaseCandidate {
        identity: id("release", key),
        title: "Edition".into(),
        artist: "Artist".into(),
        date: date.into(),
        country: String::new(),
        status: "Official".into(),
        comment: comment.into(),
        barcode: String::new(),
        labels: vec![],
        formats: vec!["CD".into()],
        disc_count: discs,
        track_count: 12,
    }
}
#[test]
fn ranking_is_stable_original_official_and_conservative() {
    let normal = edition("a", "2005-05-23", "", 1);
    let deluxe = edition("b", "2025-05-23", "Deluxe expanded anniversary", 3);
    let mut bootleg = edition("c", "2005-05-11", "", 1);
    bootleg.status = "Bootleg".into();
    let same_era_deluxe = edition("d", "2005-05-11", "Deluxe", 2);
    let mut unknown = edition("e", "", "", 1);
    unknown.track_count = 0;
    let mut values = vec![deluxe, bootleg, same_era_deluxe, unknown, normal.clone()];
    assert_eq!(
        representative_release(&album(), &values).unwrap().identity,
        normal.identity
    );
    values.reverse();
    assert_eq!(
        representative_release(&album(), &values).unwrap().identity,
        normal.identity
    );
    values.push(edition("0", "2005-05-23", "", 1));
    assert_eq!(
        representative_release(&album(), &values)
            .unwrap()
            .identity
            .external_id,
        "0"
    );
    assert!(representative_release(&album(), &[]).is_none());
}
struct Provider {
    calls: Arc<Mutex<Vec<&'static str>>>,
    incomplete_editions: bool,
}
impl CatalogProvider for Provider {
    fn search_albums(&mut self, _: &str, _: u32) -> Result<Page<AlbumCandidate>, CatalogError> {
        self.calls.lock().unwrap().push("search");
        Ok(Page {
            items: vec![album()],
            next_offset: None,
        })
    }
    fn releases(
        &mut self,
        _: &ExternalIdentity,
        _: u32,
    ) -> Result<Page<ReleaseCandidate>, CatalogError> {
        self.calls.lock().unwrap().push("editions");
        Ok(Page {
            items: vec![
                edition("normal", "2005", "", 1),
                edition("deluxe", "2025", "Deluxe", 3),
            ],
            next_offset: self.incomplete_editions.then_some(2),
        })
    }
    fn representative_releases(
        &mut self,
        _: &ExternalIdentity,
    ) -> Result<Page<ReleaseCandidate>, CatalogError> {
        self.calls.lock().unwrap().push("representative");
        Ok(Page {
            items: vec![edition("normal", "2005", "", 1)],
            next_offset: None,
        })
    }
    fn release(&mut self, id: &ExternalIdentity) -> Result<Release, CatalogError> {
        self.calls.lock().unwrap().push("lookup");
        Ok(Release {
            album: album().album(),
            identity: id.clone(),
            identities: vec![],
            title: "Edition title".into(),
            date: "2025".into(),
            credits: vec![],
            media: vec![],
        })
    }
}
#[test]
fn search_is_discovery_only_and_explicit_add_or_editions_reuse_session_requests() {
    let calls = Arc::new(Mutex::new(vec![]));
    let mut session = CatalogSession::new(Provider {
        calls: calls.clone(),
        incomplete_editions: false,
    });
    assert!(calls.lock().unwrap().is_empty());
    let result = session
        .search_albums("Friendly", 0)
        .unwrap()
        .items
        .remove(0);
    assert_eq!(*calls.lock().unwrap(), ["search"]);
    let release = session.add_album(&result).unwrap();
    assert_eq!(release.identity.external_id, "normal");
    assert_eq!(release.album.title, "Friendly");
    assert_eq!(
        *calls.lock().unwrap(),
        ["search", "representative", "lookup"]
    );
    session.add_album(&result).unwrap();
    assert_eq!(calls.lock().unwrap().len(), 3);
    session.editions(&result, 0).unwrap();
    assert_eq!(calls.lock().unwrap().len(), 4);
    session.edition(&result, &id("release", "deluxe")).unwrap();
    assert_eq!(calls.lock().unwrap().len(), 5);
    let calls = Arc::new(Mutex::new(vec![]));
    let mut session = CatalogSession::new(Provider {
        calls: calls.clone(),
        incomplete_editions: false,
    });
    session.editions(&result, 0).unwrap();
    assert_eq!(*calls.lock().unwrap(), ["editions"]);
    session.add_album(&result).unwrap();
    assert_eq!(*calls.lock().unwrap(), ["editions", "lookup"]);
}

#[test]
fn an_incomplete_rich_page_does_not_replace_default_candidate_discovery() {
    let calls = Arc::new(Mutex::new(vec![]));
    let mut session = CatalogSession::new(Provider {
        calls: calls.clone(),
        incomplete_editions: true,
    });
    let album = album();
    assert_eq!(session.editions(&album, 0).unwrap().next_offset, Some(2));
    session.add_album(&album).unwrap();
    assert_eq!(
        *calls.lock().unwrap(),
        ["editions", "representative", "lookup"]
    );
    session.editions(&album, 0).unwrap();
    session.add_album(&album).unwrap();
    assert_eq!(calls.lock().unwrap().len(), 3);
}

#[test]
fn media_counts_still_exclude_empty_candidates_and_break_original_era_ties() {
    let normal = edition("z", "2005", "", 1);
    let multi = edition("a", "2005", "", 2);
    let mut empty = edition("0", "2005", "", 1);
    empty.track_count = 0;
    assert_eq!(
        representative_release(&album(), &[multi, empty, normal])
            .unwrap()
            .identity
            .external_id,
        "z"
    );
}
