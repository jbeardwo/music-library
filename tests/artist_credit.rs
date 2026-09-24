use music_library::{
    artist_credit::{self, Compatibility::*},
    domain::ExternalIdentity,
    edition::ArtistEvidence,
};
fn artist(name: &str) -> ArtistEvidence {
    ArtistEvidence {
        name: name.into(),
        ..Default::default()
    }
}
#[test]
fn structured_credits_ignore_rendering_not_artist_names_or_identity() {
    for join in [" feat. ", " featuring ", " & ", " with "] {
        let mut a = artist("A");
        a.join_phrase = join.into();
        assert_eq!(
            artist_credit::compare(&[a, artist("B")], &[artist("A"), artist("B")]),
            Equivalent
        );
    }
    assert_eq!(
        artist_credit::compare(&[artist("A & B")], &[artist("A"), artist("B")]),
        Contradictory
    );
    assert_eq!(
        artist_credit::compare(&[artist("A"), artist("B")], &[artist("A"), artist("C")]),
        DifferentAdditional
    );
    assert_eq!(
        artist_credit::compare(&[artist("A"), artist("B")], &[artist("A")]),
        Incomplete
    );
    assert_eq!(
        artist_credit::compare(&[artist("A")], &[artist("Unrelated")]),
        Contradictory
    );
    let known = |name: &str, id: &str| ArtistEvidence {
        name: name.into(),
        identities: vec![ExternalIdentity {
            provider: "provider".into(),
            kind: "artist".into(),
            external_id: id.into(),
        }],
        ..Default::default()
    };
    assert_eq!(
        artist_credit::compare(
            &[known("A", "a"), known("B", "b")],
            &[known("A", "a"), known("B", "different")]
        ),
        Contradictory
    );
    assert!(artist_credit::same_artist(
        &known("Alias", "same"),
        &known("Canonical", "same")
    ));
    assert!(!artist_credit::same_artist(
        &known("Same name", "one"),
        &known("Same name", "two")
    ));
}
#[test]
fn featured_title_suffix_requires_its_own_structured_artist_evidence() {
    let credits = vec![
        artist("Gorillaz"),
        artist("Shaun Ryder"),
        artist("Roses Gabor"),
    ];
    let title = "DARE (feat. Shaun Ryder & Roses Gabor)";
    assert_eq!(artist_credit::musical_title(title, &credits), "DARE");
    for other in [
        "DARE (feat. Unrelated)",
        "DARE (live)",
        "DARE (remix)",
        "DARE (feat. Shaun Ryder & Unknown)",
    ] {
        assert_eq!(artist_credit::musical_title(other, &credits), other);
    }
    assert_eq!(artist_credit::musical_title(title, &[]), title);
    assert_ne!(
        artist_credit::musical_title("Super Fx", &credits),
        artist_credit::musical_title("Super Fxx", &credits)
    );
    assert_eq!(credits[2].name, "Roses Gabor");
}
