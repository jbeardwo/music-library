//! Import-only latency audit; temporary SQLite databases, no network or user data writes.
use music_library::{
    Library,
    catalog::{Album, Credit, Medium, Release, Track},
    domain::{ExternalIdentity, SearchRequest},
};
use std::time::Instant;
fn id(kind: &str, value: String) -> ExternalIdentity {
    ExternalIdentity {
        provider: "audit".into(),
        kind: kind.into(),
        external_id: value,
    }
}
fn fixture(n: usize, run: usize) -> Release {
    let credits = vec![
        Credit {
            name: "Artist A".into(),
            join_phrase: " feat. ".into(),
        },
        Credit {
            name: "Artist B".into(),
            join_phrase: "".into(),
        },
    ];
    Release {
        album: Album {
            identity: id("album", format!("{n}-{run}")),
            title: format!("Audit Album {n}"),
            date: "2005".into(),
            credits: credits.clone(),
        },
        identity: id("release", format!("{n}-{run}")),
        identities: vec![],
        title: "Edition".into(),
        date: "2005".into(),
        credits: credits.clone(),
        media: vec![Medium {
            position: 1,
            tracks: (0..n)
                .map(|i| Track {
                    position: i as u32 + 1,
                    title: format!("Audit Track {i}"),
                    credits: credits.clone(),
                    identities: vec![
                        id("track", format!("{n}-{run}-{i}")),
                        id("recording", format!("{n}-{run}-{i}")),
                        ExternalIdentity {
                            provider: "isrc".into(),
                            kind: "recording".into(),
                            external_id: format!("ISRC{i}A"),
                        },
                        ExternalIdentity {
                            provider: "isrc".into(),
                            kind: "recording".into(),
                            external_id: format!("ISRC{i}B"),
                        },
                    ],
                })
                .collect(),
        }],
    }
}
fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let seed = std::env::args().nth(1);
    for n in [5, 15, 30, 100] {
        let mut fresh = vec![];
        let mut warm = vec![];
        let mut search = vec![];
        for run in 0..11 {
            let temp = tempfile::tempdir()?;
            let path = temp.path().join("library.sqlite");
            // Optional seed must be a closed/checkpointed diagnostic database, never opened here.
            if let Some(seed) = &seed {
                std::fs::copy(seed, &path)?;
            }
            let mut library = Library::open(path)?;
            let input = fixture(n, run);
            let t = Instant::now();
            let imported = library.add_catalog_release(&input)?;
            fresh.push(t.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(imported.track_ids.len(), n);
            let t = Instant::now();
            library.search(&SearchRequest {
                text: input.album.title.clone(),
                limit: 21,
                ..Default::default()
            })?;
            search.push(t.elapsed().as_secs_f64() * 1000.0);
            let t = Instant::now();
            assert_eq!(library.add_catalog_release(&input)?, imported);
            warm.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        println!(
            "tracks={n} repetitions=11 import_median_ms={:.3} reimport_median_ms={:.3} search_median_ms={:.3}",
            median(&mut fresh),
            median(&mut warm),
            median(&mut search)
        );
    }
    Ok(())
}
