//! Runs normal application discovery on pre-created disposable copies only.
use music_library::{Library, catalog::CatalogProvider};
use serde_json::json;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("/tmp/mb-real-*.sqlite required")?;
    if !path.starts_with("/tmp/mb-real-") {
        return Err("disposable copy required".into());
    }
    let mut lib = Library::open(&path)?;
    let before = lib.connection_coverage("musicbrainz")?;
    let pending = lib.pending_musicbrainz_policy(256)?;
    let explicit_reevaluation = std::env::args().nth(2).as_deref() == Some("--all-unresolved");
    let groups = lib
        .connection_groups("musicbrainz", false, None, 200)?
        .into_iter()
        .filter(|g| explicit_reevaluation || pending.contains(&g.id))
        .collect::<Vec<_>>();
    let mut provider = music_library_musicbrainz::MusicBrainz::new();
    assert!(provider.structured_group_discovery());
    let mut rows = vec![];
    for g in groups {
        eprintln!("Discovering {} — {}", g.artist, g.title);
        let identities = lib.list_album_external_identities(&g.id)?;
        let requests_before = provider.request_count();
        let counts_before = provider.request_counts();
        let result = lib.reconcile_musicbrainz(&g.id, &mut provider);
        let snapshot = lib.retained_musicbrainz_discovery(&g.id)?;
        let review = lib.musicbrainz_review(&g.id)?;
        let projections = review.candidates.iter().filter_map(|c|c.programs.as_ref().or(c.inspection.as_ref()).map(|p|serde_json::json!({"group":c.metadata.identity,"title":c.metadata.title,"review":lib.musicbrainz_program_review(&g.id,p).unwrap()}))).collect::<Vec<_>>();
        let after = lib.connection_tracks("musicbrainz", false, &g.id, None, 200)?;
        rows.push(json!({"requests":provider.request_count()-requests_before,"request_classes":provider.request_counts().into_iter().map(|(k,v)|{let before=counts_before.get(&k).copied().unwrap_or(0);(k,v-before)}).collect::<std::collections::BTreeMap<_,_>>(),"artist":g.artist,"album":g.title,"id":g.id,"before_connected":g.matched,"total":g.total,"identities_before":identities,"identities_after":lib.list_album_external_identities(&g.id)?,"outcome":result.as_ref().map(|r|format!("{:?}",r.outcome)).unwrap_or_else(|e|e.to_string()),"discovery":snapshot,"review":review,"remaining":after,"projections":projections}));
        let report = json!({"path":path,"before":before,"after":lib.connection_coverage("musicbrainz")?,"albums":rows});
        std::fs::write(format!("{path}.json"), serde_json::to_vec_pretty(&report)?)?;
    }
    println!(
        "{}",
        serde_json::to_string(
            &json!({"before":before,"after":lib.connection_coverage("musicbrainz")?})
        )?
    );
    Ok(())
}
