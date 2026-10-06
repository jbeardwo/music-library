//! Bounded diagnostic path on the existing 200k fixture. No provider requests.
use music_library::{
    Library,
    browse::Request,
    catalog::Page,
    domain::*,
    song_resolution::{Candidate, evaluate},
};
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let l = Library::open(
        std::env::args()
            .nth(1)
            .unwrap_or("target/performance/library-200k.sqlite".into()),
    )?;
    let track = TrackId(
        l.browse(&Request {
            limit: 1,
            ..Default::default()
        })?[0]
            .id
            .clone(),
    );
    let mut reads = vec![];
    let mut evaluations = vec![];
    for _ in 0..1000 {
        let t = Instant::now();
        let input = l.song_resolution_input(&track)?;
        let ids = l.diagnostic_track_identities(&track)?;
        reads.push(t.elapsed());
        let c = Candidate {
            album_identity: None,
            identity: ExternalIdentity {
                provider: "spotify".into(),
                kind: "track".into(),
                external_id: "synthetic".into(),
            },
            title: input.title.clone(),
            artist: input.artist.clone(),
            artists: input.artists.clone(),
            album: input.album.clone(),
            date: "".into(),
            album_artists: input.album_artists.clone(),
            album_type: "album".into(),
            album_total_tracks: Some(input.album_required_tracks as u32),
            duration_ms: input.duration_ms.unwrap_or(0),
            disc: input.disc.unwrap_or(1),
            number: input.number.unwrap_or(1),
        };
        let page = Page {
            items: vec![c; 10],
            next_offset: None,
        };
        let t = Instant::now();
        for i in 0..10 {
            std::hint::black_box(evaluate(&input, &page, i, &ids));
        }
        evaluations.push(t.elapsed());
    }
    reads.sort();
    evaluations.sort();
    println!(
        "200k: targeted input/Artist equivalence/identity context p50 {:?}, p95 {:?}; ten candidate traces p50 {:?}, p95 {:?}",
        reads[500], reads[950], evaluations[500], evaluations[950]
    );
    Ok(())
}
