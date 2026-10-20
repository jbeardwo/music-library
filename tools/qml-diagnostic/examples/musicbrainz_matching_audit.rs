//! Bounded read/evaluate audit. Only opens a disposable SQLite backup. Does not apply matches.
use music_library::{
    Library,
    album_matching::{Preparation, accepted_album, resolve_artist},
    catalog::CatalogProvider,
    domain::AlbumId,
};
use serde_json::json;
use std::{fs, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let db = args.get(1).ok_or("database required")?;
    if !db.starts_with("/tmp/mb-audit-") {
        return Err("use /tmp/mb-audit- disposable database".into());
    }
    let out = args.get(2).ok_or("output directory required")?;
    fs::create_dir_all(out)?;
    let lib = Library::open(db)?;
    let sql = rusqlite::Connection::open(db)?;
    let groups=sql.prepare("SELECT DISTINCT r.album_id,a.title,a.match_artist_credit FROM library_membership m JOIN track t ON t.id=m.track_id JOIN release r ON r.id=t.release_id JOIN effective_album_metadata a ON a.album_id=r.album_id WHERE NOT EXISTS(SELECT 1 FROM connected_musicbrainz_track c WHERE c.track_id=t.id) ORDER BY a.match_artist_credit,a.title")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut mb = music_library_musicbrainz::MusicBrainz::new();
    if args.get(3).is_some_and(|a| a == "--analyze") {
        let mut rows = vec![];
        for entry in fs::read_dir(out)? {
            let file = entry?.path();
            if file.extension().is_none_or(|s| s != "json") {
                continue;
            }
            let d: serde_json::Value = serde_json::from_slice(&fs::read(&file)?)?;
            let Some(id) = d["album_id"].as_str().or(d["request"]["album"].as_str()) else {
                continue;
            };
            if d["programs"].is_null() {
                continue;
            }
            let programs: music_library::album_program::Programs =
                serde_json::from_value(d["programs"].clone())?;
            let title = d["title"]
                .as_str()
                .or(d["request"]["title"].as_str())
                .unwrap_or("");
            let mapped:Option<String>=sql.query_row("SELECT a.album_id FROM effective_album_metadata a WHERE a.title=?1 AND EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id=a.album_id) LIMIT 1",[title],|r|r.get(0)).ok();
            let Some(mapped) = mapped else { continue };
            let local = lib.local_album_tracks(&AlbumId(mapped.clone()))?;
            if local.is_empty() {
                continue;
            }
            let checks=programs.programs.iter().map(|p|json!({"fit":format!("{:?}",music_library::album_program::program_fit(&local,p)),"tracks":local.iter().map(|l|json!({"title":l.evidence.title,"candidates":p.tracks.iter().enumerate().filter_map(|(j,t)|{let c=music_library::album_program::inspect_candidate(l,t,j);(c.position||c.exact_title||c.trusted_identity).then(||json!({"title":t.title,"check":format!("{c:?}")}))}).collect::<Vec<_>>()})).collect::<Vec<_>>()})).collect::<Vec<_>>();
            let complete = music_library::album_program::Programs {
                programs: programs
                    .programs
                    .iter()
                    .filter(|p| p.complete)
                    .cloned()
                    .collect(),
                ..programs.clone()
            };
            rows.push(json!({"file":file,"evaluated_album":mapped,"original_album":id,"selected":music_library::album_program::selected_programs(&local,&programs),"checks":checks,"complete_only_comparison":music_library::album_program::compare_album(&local,&complete).iter().map(|o|format!("{o:?}")).collect::<Vec<_>>()}));
        }
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if let Some(plan) = args.get(3) {
        let plan: serde_json::Value = serde_json::from_slice(&fs::read(plan)?)?;
        for item in plan.as_array().ok_or("plan array required")? {
            let name = item["name"].as_str().ok_or("name required")?;
            let file = format!("{out}/{name}.json");
            if Path::new(&file).exists() {
                continue;
            }
            let mut result = json!({"request":item});
            if let Some(q) = item["query"].as_str() {
                result["page"] = json!(format!(
                    "{:?}",
                    mb.search_albums(q, item["offset"].as_u64().unwrap_or(0) as u32)
                ));
            }
            if let Some(group) = item["group"].as_str() {
                let g = music_library::domain::ExternalIdentity {
                    provider: "musicbrainz".into(),
                    kind: "release_group".into(),
                    external_id: group.into(),
                };
                match mb.album_programs(&g) {
                    Ok(p) => {
                        if let Some(id) = item["album"].as_str() {
                            let local = lib.local_album_tracks(&AlbumId(id.into()))?;
                            result["compare_album"] = json!(
                                music_library::album_program::compare_album(&local, &p)
                                    .iter()
                                    .map(|o| format!("{o:?}"))
                                    .collect::<Vec<_>>()
                            );
                            result["inspections"] = json!(
                                p.programs
                                    .iter()
                                    .map(|p| music_library::album_program::inspect_album_program(
                                        &local,
                                        p,
                                        item["title"].as_str().unwrap_or("")
                                    ))
                                    .collect::<Vec<_>>()
                            );
                        }
                        result["programs"] = json!(p);
                    }
                    Err(e) => result["error"] = json!(format!("{e:?}")),
                }
            }
            fs::write(file, serde_json::to_vec_pretty(&result)?)?;
            println!("DONE {name}");
        }
        return Ok(());
    }

    for (id, title, artist) in groups {
        let file = format!("{out}/{id}.json");
        if Path::new(&file).exists() {
            println!("REUSE {title}");
            continue;
        }
        let album = AlbumId(id.clone());
        let local = lib.local_album_tracks(&album)?;
        let mut result = json!({"album_id":id,"title":title,"artist":artist,"local":local.iter().map(|l|json!({"track_id":l.track_id,"evidence":l.evidence})).collect::<Vec<_>>(),"calls":[]});
        let mut group = lib
            .list_album_external_identities(&album)?
            .into_iter()
            .find(|i| i.provider == "musicbrainz" && i.kind == "release_group");
        let preparation = lib.prepare_album_match(&album)?;
        result["preparation"] = json!(format!("{preparation:?}"));
        if group.is_none() {
            if let Preparation::Ready(input) = preparation {
                let mut aid = input.known_artist.clone();
                if aid.is_none() {
                    result["calls"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"search_artists":input.artist}));
                    match mb.search_artists(&input.artist) {
                        Ok(page) => {
                            result["artist_page"] = json!(format!("{page:?}"));
                            match resolve_artist(&input.artist, &page) {
                                Ok(a) => aid = Some(a),
                                Err(e) => result["artist_resolution"] = json!(format!("{e:?}")),
                            }
                        }
                        Err(e) => result["error"] = json!(format!("{e:?}")),
                    }
                }
                if let Some(a) = aid {
                    result["artist_identity"] = json!(a);
                    result["calls"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"artist_albums":{"artist":a,"title":input.title}}));
                    match mb.artist_albums(&a, &input.title) {
                        Ok(page) => {
                            let outcome = accepted_album(&input.title, &a, &page);
                            result["automatic_candidates"] = json!(format!("{page:?}"));
                            result["automatic_outcome"] = json!(format!("{outcome:?}"));
                            if let music_library::album_matching::MatchOutcome::Matched(g)
                            | music_library::album_matching::MatchOutcome::MatchedClose(g) =
                                outcome
                            {
                                group = Some(g)
                            }
                        }
                        Err(e) => result["error"] = json!(format!("{e:?}")),
                    }
                }
            }
            // One user-equivalent raw search per unresolved Album, never auto-accept it.
            let q = format!(
                "releasegroup:\"{}\" AND artist:\"{}\"",
                title.replace('"', "\\\""),
                artist.replace('"', "\\\"")
            );
            result["calls"]
                .as_array_mut()
                .unwrap()
                .push(json!({"manual_search_albums":q}));
            match mb.search_albums(&q, 0) {
                Ok(page) => {
                    result["manual_page"] = json!(format!("{page:?}"));
                    if group.is_none() {
                        group = page.items.first().map(|a| a.identity.clone());
                        result["program_is_manual_first_unconfirmed"] = json!(true)
                    }
                }
                Err(e) => result["manual_error"] = json!(format!("{e:?}")),
            }
        }
        if let Some(g) = group {
            result["group"] = json!(g);
            result["calls"]
                .as_array_mut()
                .unwrap()
                .push(json!({"album_programs":g}));
            match mb.album_programs(&g) {
                Ok(p) => {
                    result["programs"] = json!(p);
                    result["compare_album"] = json!(
                        music_library::album_program::compare_album(&local, &p)
                            .iter()
                            .map(|x| format!("{x:?}"))
                            .collect::<Vec<_>>()
                    );
                    result["inspections"] = json!(
                        p.programs
                            .iter()
                            .map(|x| music_library::album_program::inspect_album_program(
                                &local, x, &title
                            ))
                            .collect::<Vec<_>>()
                    );
                }
                Err(e) => result["program_error"] = json!(format!("{e:?}")),
            }
            result["calls"]
                .as_array_mut()
                .unwrap()
                .push(json!({"recordings":g}));
            match mb.recordings(&g) {
                Ok(page) => {
                    result["recording_page"] = json!(format!("{page:?}"));
                    result["recording_comparisons"]=json!(local.iter().map(|l|{
     let r=music_library::recording::LocalTrack{track_id:l.track_id.clone(),recording_id:lib.recording_for_track(&l.track_id).unwrap().recording_id,title:l.evidence.title.clone().unwrap_or_default(),artist:l.evidence.artists.iter().map(|a|a.name.clone()).collect::<Vec<_>>().join(", "),artist_ids:l.evidence.artists.iter().flat_map(|a|a.identities.iter().filter(|i|i.provider=="musicbrainz"&&i.kind=="artist").cloned()).collect(),duration_ms:l.evidence.duration_ms,known:l.evidence.recording.identities.iter().any(|i|i.provider=="musicbrainz"&&i.kind=="recording"&&!i.external_id.is_empty())};json!({"track":l.track_id,"title":r.title,"outcome":format!("{:?}",music_library::recording::compare(&r,&page))})
    }).collect::<Vec<_>>());
                }
                Err(e) => result["recording_error"] = json!(format!("{e:?}")),
            }
        }
        fs::write(file, serde_json::to_vec_pretty(&result)?)?;
        println!("DONE {artist} / {title}");
    }
    Ok(())
}
