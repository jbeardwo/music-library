//! Offline measurements on a disposable copy. Bulk confirmation changes that copy.
use music_library::{
    Library,
    album_program::{Program, Programs},
    browse::{Pane, Request},
    domain::ExternalIdentity,
};
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("DISPOSABLE_DATABASE")?;
    let mut l = Library::open(&path)?;
    let start = Instant::now();
    let groups = l.spotify_review_groups(None, 200)?;
    println!(
        "First grouped page: {} Albums {:?}",
        groups.len(),
        start.elapsed()
    );
    let start = Instant::now();
    let count = l.unresolved_spotify_count()?;
    println!("Unresolved count: {count} {:?}", start.elapsed());
    if let Some(group) = groups.first() {
        let start = Instant::now();
        let tracks = l.browse(&Request {
            pane: Pane::Songs,
            album: Some(group.id.clone()),
            unresolved_spotify: true,
            limit: 201,
            ..Default::default()
        })?;
        println!("Expand Album: {} rows {:?}", tracks.len(), start.elapsed());
        let start = Instant::now();
        let mut local = l.local_album_tracks(&group.id)?;
        println!(
            "Canonical program lookup: {} rows {:?}",
            local.len(),
            start.elapsed()
        );
        for (n, t) in local.iter_mut().enumerate() {
            t.evidence.identities = vec![ExternalIdentity {
                provider: "spotify".into(),
                kind: "track".into(),
                external_id: format!("measurement{n:011}"),
            }];
        }
        let programs = Programs {
            album: ExternalIdentity {
                provider: "spotify".into(),
                kind: "album".into(),
                external_id: "measurement-album".into(),
            },
            programs: vec![Program {
                identity: None,
                complete: true,
                tracks: local.into_iter().map(|t| t.evidence).collect(),
            }],
            note: String::new(),
        };
        let plan = l.prepare_spotify_album_confirmation(&group.id, &programs)?;
        let start = Instant::now();
        let connected = l.confirm_spotify_album(&plan, true)?;
        println!("Bulk connect: {connected} Tracks {:?}", start.elapsed());
        let start = Instant::now();
        let remaining = l.browse(&Request {
            pane: Pane::Songs,
            album: Some(group.id.clone()),
            unresolved_spotify: true,
            limit: 201,
            ..Default::default()
        })?;
        println!(
            "Targeted Album update: {} unresolved rows {:?}",
            remaining.len(),
            start.elapsed()
        );
        let start = Instant::now();
        let refreshed = l.spotify_review_groups(None, 200)?;
        println!(
            "Bounded group update read: {} groups {:?}",
            refreshed.len(),
            start.elapsed()
        );
    }
    println!("Provider requests: 0");
    Ok(())
}
