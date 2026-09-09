//! Opt-in live comparison using the production transport, retries and rate gate.
use super::*;

#[test]
#[ignore = "explicit live MusicBrainz request-shape audit"]
fn live_request_shapes() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::PathBuf::from(std::env::var("MUSIC_LIBRARY_SHAPE_OUTPUT")?);
    std::fs::create_dir_all(&directory)?;
    let mut client = MusicBrainz::new();
    let queries = [
        ("animals", "releasegroup:Animals AND artist:\"Pink Floyd\""),
        (
            "demon-days",
            "releasegroup:\"Demon Days\" AND artist:Gorillaz",
        ),
        ("lava-land", "releasegroup:\"Lava Land\" AND artist:Piglet"),
        (
            "abbey-road",
            "releasegroup:\"Abbey Road\" AND artist:\"The Beatles\"",
        ),
    ];
    let mut albums = Vec::new();
    for (name, query) in queries {
        if std::env::var("MUSIC_LIBRARY_SHAPE_ALBUM").is_ok_and(|only| only != name) {
            continue;
        }
        let results = client.search_albums(query, 0)?.items;
        let album = results
            .iter()
            .find(|a| a.primary_type == "Album")
            .or_else(|| results.first())
            .ok_or("No search result")?
            .clone();
        eprintln!(
            "shape-album name={name} id={} title={} artist={} date={}",
            album.identity.external_id, album.title, album.artist, album.date
        );
        albums.push((name, album));
    }
    let shapes = [
        ("A", Some("artist-credits+labels+media"), false),
        ("B", Some("media"), false),
        ("C", None, false),
        ("D", None, true),
        ("F", Some("media"), true),
        ("E", None, true),
    ];
    // Rotate shape order, and reverse Album order each round. No parallel requests.
    for round in 0..3 {
        for step in 0..albums.len() {
            let index = if round % 2 == 0 {
                step
            } else {
                albums.len() - 1 - step
            };
            let (name, album) = &albums[index];
            for shift in 0..shapes.len() {
                let (shape, inc, official) = shapes[(shift + round * 2 + index) % shapes.len()];
                if std::env::var("MUSIC_LIBRARY_SHAPE").is_ok_and(|only| only != shape) {
                    continue;
                }
                let mut params = vec![
                    ("release-group", album.identity.external_id.clone()),
                    ("limit", "100".into()),
                    ("offset", "0".into()),
                ];
                if let Some(inc) = inc {
                    params.push(("inc", inc.into()));
                }
                if official {
                    params.push(("status", "official".into()));
                }
                if shape == "E" {
                    params = vec![
                        (
                            "query",
                            format!("rgid:{} AND status:official", album.identity.external_id),
                        ),
                        ("limit", "100".into()),
                        ("offset", "0".into()),
                    ];
                }
                eprintln!("shape-begin album={name} round={round} shape={shape}");
                let start = Instant::now();
                match client.request::<serde_json::Value>("release", &params) {
                    Ok(value) => {
                        eprintln!(
                            "shape-end album={name} round={round} shape={shape} total_ms={:.3} returned={} count={}",
                            start.elapsed().as_secs_f64() * 1000.,
                            value["releases"].as_array().map_or(0, Vec::len),
                            value
                                .get("release-count")
                                .or_else(|| value.get("count"))
                                .unwrap_or(&serde_json::Value::Null)
                        );
                        std::fs::write(
                            directory.join(format!("{name}-{shape}-{round}.json")),
                            serde_json::to_vec_pretty(&value)?,
                        )?;
                    }
                    Err(error) => eprintln!(
                        "shape-end album={name} round={round} shape={shape} total_ms={:.3} error={error}",
                        start.elapsed().as_secs_f64() * 1000.
                    ),
                }
            }
        }
    }
    Ok(())
}

/// Final endpoint audit: no candidate browse or persistence; the production HTTP
/// transport, timeout, retry policy and process-wide gate are used unchanged.
#[test]
#[ignore = "explicit live MusicBrainz search/detail audit"]
fn live_search_detail_shapes() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::PathBuf::from(std::env::var("MUSIC_LIBRARY_ENDPOINT_OUTPUT")?);
    std::fs::create_dir_all(&directory)?;
    let client = MusicBrainz::new();
    let searches = [
        (
            "demon-days",
            "releasegroup:\"Demon Days\" AND artist:Gorillaz",
        ),
        ("animals", "releasegroup:Animals AND artist:\"Pink Floyd\""),
        ("ambiguous-animals", "Animals"),
    ];
    let releases = [
        ("animals", "0a567b61-f09d-4549-9d43-1c3e81c21b26"),
        ("demon-days", "14190090-8b00-4c4a-a861-50d7734c16fd"),
    ];
    let variants = [
        (
            "current",
            "release-groups+recordings+artist-credits+isrcs+media",
        ),
        ("no-isrc", "release-groups+recordings+artist-credits+media"),
        ("no-group", "recordings+artist-credits+isrcs+media"),
        ("no-media", "release-groups+recordings+artist-credits+isrcs"),
        ("minimal", "recordings+artist-credits"),
    ];
    for round in 0..3 {
        for step in 0..searches.len() {
            let index = if round % 2 == 0 {
                step
            } else {
                searches.len() - 1 - step
            };
            let (name, query) = searches[index];
            for shift in 0..3 {
                let limit = [5, 10, 25][(shift + round + index) % 3];
                endpoint_sample(
                    &client,
                    &directory,
                    &format!("search-{name}-{limit}-{round}"),
                    "release-group",
                    &[
                        ("query", query.into()),
                        ("limit", limit.to_string()),
                        ("offset", "0".into()),
                    ],
                )?;
            }
        }
        for step in 0..releases.len() {
            let index = if round % 2 == 0 {
                step
            } else {
                releases.len() - 1 - step
            };
            let (name, id) = releases[index];
            for shift in 0..variants.len() {
                let (shape, inc) = variants[(shift + round * 2 + index) % variants.len()];
                endpoint_sample(
                    &client,
                    &directory,
                    &format!("lookup-{name}-{shape}-{round}"),
                    &format!("release/{id}"),
                    &[("inc", inc.into())],
                )?;
            }
        }
    }
    Ok(())
}

fn endpoint_sample(
    client: &MusicBrainz,
    directory: &std::path::Path,
    label: &str,
    path: &str,
    params: &[(&str, String)],
) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("endpoint-begin label={label}");
    let start = Instant::now();
    match client.request::<serde_json::Value>(path, params) {
        Ok(value) => {
            eprintln!(
                "endpoint-end label={label} total_ms={:.3}",
                start.elapsed().as_secs_f64() * 1000.
            );
            std::fs::write(
                directory.join(format!("{label}.json")),
                serde_json::to_vec_pretty(&value)?,
            )?;
        }
        Err(error) => eprintln!(
            "endpoint-end label={label} total_ms={:.3} error={error}",
            start.elapsed().as_secs_f64() * 1000.
        ),
    }
    Ok(())
}
