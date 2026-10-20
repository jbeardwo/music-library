//! Offline final-policy replay and compact report of normal discovery observations.
//! Requires copies produced by the MusicBrainz structured_validation example.
use music_library::{Library, catalog::*, domain::*};
use rusqlite::{Connection, OpenFlags};
use serde_json::json;
struct Offline;
impl CatalogProvider for Offline {
    fn search_albums(&mut self, _: &str, _: u32) -> Result<Page<AlbumCandidate>, CatalogError> {
        panic!("retained replay must not request provider")
    }
    fn releases(
        &mut self,
        _: &ExternalIdentity,
        _: u32,
    ) -> Result<Page<ReleaseCandidate>, CatalogError> {
        panic!("retained replay must not request provider")
    }
    fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
        panic!("retained replay must not request provider")
    }
    fn artist_albums(
        &mut self,
        _: &ExternalIdentity,
        _: &str,
    ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
        panic!("retained replay must not request provider")
    }
    fn artist_release_groups(
        &mut self,
        _: &ExternalIdentity,
        _: u32,
    ) -> Result<Page<AlbumCandidate>, CatalogError> {
        panic!("retained replay must not request provider")
    }
    fn album_programs(
        &mut self,
        _: &ExternalIdentity,
    ) -> Result<music_library::album_program::Programs, CatalogError> {
        panic!("retained replay must not request provider")
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let prefix = std::env::args()
        .nth(1)
        .ok_or("/tmp/mb-phase3-final prefix required")?;
    if !prefix.starts_with("/tmp/mb-phase3-") {
        return Err("disposable prefix required".into());
    }
    let mut datasets = vec![];
    for name in ["fresh", "profile"] {
        let path = format!("{prefix}-{name}.sqlite");
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(format!("{path}.json"))?)?;
        let baseline_path = format!("{prefix}-summary-baseline-{name}.sqlite");
        if std::path::Path::new(&baseline_path).exists() {
            return Err("new summary baseline copy required".into());
        }
        let source = Connection::open_with_flags(
            format!("/tmp/mb-phase12-final-reasons-{name}.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        source.execute("VACUUM INTO ?1", [&baseline_path])?;
        let baseline = Library::open(&baseline_path)?;
        let mut lib = Library::open(&path)?;
        let before = baseline.connection_coverage("musicbrainz")?;
        let groups = baseline.connection_groups("musicbrainz", false, None, 200)?;
        let mut albums = vec![];
        for g in groups {
            let local = baseline.local_album_tracks(&g.id)?;
            // Same normal application path, this time with a provider that panics
            // on every discovery/program call. No Group IDs are injected.
            let resolution = lib.reconcile_musicbrainz(&g.id, &mut Offline)?;
            let s = lib
                .retained_musicbrainz_discovery(&g.id)?
                .ok_or("missing snapshot")?;
            let original = report["albums"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["id"] == json!(g.id))
                .unwrap();
            let mut candidates = vec![];
            for row in &resolution.report.candidates {
                let origin=s.title_page.as_ref().and_then(|p|p.items.iter().position(|c|c.identity==row.identity).map(|n|json!({"path":"artist_scoped_title_search","provider_rank":n+1}))).or_else(||s.pages.iter().find_map(|p|p.page.items.iter().position(|c|c.identity==row.identity).map(|n|json!({"path":"artist_catalog_browse","page":p.offset/10+1,"offset":p.offset,"page_rank":n+1,"catalog_rank":p.offset as usize+n+1}))));
                let p = s.programs.iter().find(|p| p.album == row.identity);
                let assessment =
                    p.map(|p| music_library::musicbrainz_recording::assess(&local, p, &g.title));
                let mappings=assessment.as_ref().map(|a|a.programs.iter().map(|c|json!({"anchors":c.program.anchors,"strong_alignment":c.program.alignment.strong,"rows":c.program.tracks.iter().map(|t|json!({"track":t.track_id,"canonical":t.local.title,"provider":t.provider.as_ref().and_then(|p|p.title.clone()),"decision":t.decision})).collect::<Vec<_>>()})).collect::<Vec<_>>());
                candidates.push(json!({"group":row.identity,"title":row.title.provider_raw,"artist":row.artist,"artist_ids":row.artist_ids,"date":row.provider_date,"type":row.title.provider_type,"observation":row.observation,"origin":origin,"application_rank":candidates.len()+1,"evaluation":s.evaluations.iter().find(|e|e.group==row.identity),"programs":p.map(|p|p.programs.iter().map(|p|json!({"release":p.identity,"complete":p.complete,"tracks":p.tracks.len()})).collect::<Vec<_>>()),"mappings_before_association":mappings,"recording_certainty_before_association":assessment.as_ref().map(|a|&a.recordings)}));
            }
            let review = lib.musicbrainz_review(&g.id)?;
            albums.push(json!({"artist":g.artist,"album":g.title,"album_id":g.id,"saved":g.total,"before_connected":g.matched,"after_connected":g.total-lib.connection_tracks("musicbrainz",false,&g.id,None,200)?.len() as i64,"before_identities":baseline.list_album_external_identities(&g.id)?,"after_identities":lib.list_album_external_identities(&g.id)?,"outcome":format!("{:?}",resolution.outcome),"lifecycle_reason":review.reason,"remaining":lib.connection_tracks("musicbrainz",false,&g.id,None,200)?,"actual_http_attempts":original["requests"],"request_classes":original["request_classes"],"pages_inspected":s.pages.len(),"catalog_groups_inspected":s.pages.iter().map(|p|p.page.items.len()).sum::<usize>(),"unique_groups_inspected":candidates.len(),"group_program_ensembles":s.programs.len(),"sampled_release_programs":s.programs.iter().map(|p|p.programs.len()).sum::<usize>(),"retained_replay_provider_requests":0,"candidates":candidates}));
        }
        let db = Connection::open(&path)?;
        let exact: i64 = db.query_row(
            "SELECT count(*) FROM release_external_identity WHERE provider='musicbrainz'",
            [],
            |r| r.get(0),
        )?;
        datasets.push(json!({"dataset":name,"path":path,"before":before,"after":lib.connection_coverage("musicbrainz")?,"exact_musicbrainz_release_identities":exact,"albums":albums}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"logical_request_budget":music_library::musicbrainz_discovery::MAX_REQUESTS,"max_catalog_pages":3,"max_group_ensembles":6,"datasets":datasets})
        )?
    );
    Ok(())
}
