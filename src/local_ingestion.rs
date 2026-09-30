//! One local ingestion workflow for automatic locations and explicit files/folders.
//! Discovery observations remain separate from the intentional import/membership step.
use crate::{
    Library, Result,
    domain::*,
    filesystem::{MetadataExtractor, observe},
    storage::{KnownLocalSource, bytes_to_path, path_to_bytes},
};
use rusqlite::{OptionalExtension, params};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;
const BATCH: usize = 128;

#[derive(Clone, Debug)]
pub struct Location {
    pub id: RootId,
    pub path: PathBuf,
}
#[derive(Clone, Debug)]
pub enum Request {
    Files(Vec<PathBuf>),
    Folder(PathBuf),
    Rescan(RootId),
    ConfiguredLocations,
}
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub scanned: u64,
    pub parsed: u64,
    pub unchanged: u64,
    pub imported: u64,
    pub unsupported: u64,
    pub unreadable: u64,
    pub locations_failed: u64,
    /// Successfully admitted Releases, also the ordinary post-import scheduling input.
    pub releases: Vec<ImportedRelease>,
}
#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub scanned: u64,
    pub imported: u64,
}
#[derive(Clone)]
struct Known {
    root: Option<String>,
    observation: KnownLocalSource,
    suppressed: bool,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn source_json(ids: &[SourceId]) -> String {
    serde_json::to_string(&ids.iter().map(AsRef::as_ref).collect::<Vec<_>>()).expect("source IDs")
}

impl Library {
    pub fn local_locations(&self) -> Result<Vec<Location>> {
        Ok(self
            .store
            .connection
            .prepare(
                "SELECT id,location FROM discovery_root WHERE kind='local_filesystem' ORDER BY id",
            )?
            .query_map([], |r| {
                Ok(Location {
                    id: RootId(r.get(0)?),
                    path: bytes_to_path(r.get(1)?),
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Stop scanning a location, retaining music, source records, and exclusions.
    pub fn remove_local_location(&mut self, root: &RootId) -> Result<bool> {
        let tx = self.store.connection.transaction()?;
        tx.execute(
            "DELETE FROM local_artist_context WHERE root_id=?1",
            [root.as_ref()],
        )?;
        let removed = tx.execute("DELETE FROM discovery_root WHERE id=?1", [root.as_ref()])? > 0;
        tx.commit()?;
        Ok(removed)
    }
    /// Open before spawning; no migrations or filesystem work run on the caller's thread.
    pub fn local_ingestion_worker(&self) -> Result<Worker> {
        let path = self
            .store
            .connection
            .path()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| {
                crate::Error::Invalid("Local import requires a file-backed library".into())
            })?;
        Ok(Worker(PathBuf::from(path)))
    }
    pub fn ingest_local(
        &mut self,
        request: &Request,
        extractor: &mut dyn MetadataExtractor,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Report> {
        self.store.connection.execute_batch("CREATE TEMP TABLE IF NOT EXISTS ingestion_candidates(source_id TEXT PRIMARY KEY,parent BLOB NOT NULL,title_key TEXT NOT NULL,credit_key TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS ingestion_groups ON ingestion_candidates(parent,title_key,credit_key,source_id);
            CREATE TEMP TABLE IF NOT EXISTS ingestion_sources(source_id TEXT PRIMARY KEY);
            DELETE FROM ingestion_candidates; DELETE FROM ingestion_sources;")?;
        let mut report = Report::default();
        match request {
            Request::Files(paths) => {
                let roots = self.local_locations()?;
                let mut seen = HashSet::new();
                let mut buckets = HashMap::<Option<RootId>, Vec<PathBuf>>::new();
                for path in paths {
                    let path = match path.canonicalize() {
                        Ok(p) => p,
                        Err(_) => {
                            report.unreadable += 1;
                            continue;
                        }
                    };
                    if !seen.insert(path.clone()) {
                        continue;
                    }
                    if !extractor.supports(&path) {
                        report.unsupported += 1;
                        continue;
                    }
                    // An existing location supplies evidence; selecting a file never creates one.
                    let root = roots
                        .iter()
                        .filter(|r| path.starts_with(&r.path))
                        .max_by_key(|r| r.path.components().count())
                        .map(|r| r.id.clone());
                    let bucket = buckets.entry(root.clone()).or_default();
                    bucket.push(path);
                    if bucket.len() == BATCH {
                        self.ingest_batch(
                            bucket,
                            root.as_ref(),
                            None,
                            true,
                            true,
                            extractor,
                            &mut report,
                        )?;
                        bucket.clear();
                        progress(Progress {
                            scanned: report.scanned,
                            imported: report.imported,
                        });
                    }
                }
                for (root, paths) in buckets {
                    self.ingest_batch(
                        &paths,
                        root.as_ref(),
                        None,
                        true,
                        true,
                        extractor,
                        &mut report,
                    )?;
                }
            }
            Request::Folder(path) => {
                let path = path
                    .canonicalize()
                    .map_err(|source| crate::Error::Filesystem {
                        path: path.clone(),
                        source,
                    })?;
                if !path.is_dir() {
                    return Err(crate::Error::Invalid("Choose a music folder".into()));
                }
                let root = self.register_local_root(&path)?;
                self.ingest_root(&root, true, extractor, &mut report, progress)?;
            }
            Request::Rescan(root) => {
                self.ingest_root(root, false, extractor, &mut report, progress)?
            }
            Request::ConfiguredLocations => {
                for location in self.local_locations()? {
                    self.ingest_root(&location.id, false, extractor, &mut report, progress)?;
                }
            }
        }
        self.import_candidate_groups(&mut report, progress)?;
        // The same conservative finalizer as migration backfill, scoped to all
        // successfully encountered associated Albums (including unchanged re-adds).
        let tx = self.store.connection.transaction()?;
        let albums = tx.prepare("SELECT DISTINCT r.album_id FROM ingestion_sources s CROSS JOIN track_source ts ON ts.source_id=s.source_id JOIN track t ON t.id=ts.track_id JOIN release r ON r.id=t.release_id")?
            .query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        crate::artist_identity::reconcile_albums(&tx, &albums)?;
        tx.commit()?;
        // Indexed temporary source set bounds scheduling to this operation's admitted music.
        let mut releases = BTreeMap::<String, Vec<TrackId>>::new();
        for row in self.store.connection.prepare("SELECT DISTINCT t.release_id,t.id FROM ingestion_sources s CROSS JOIN track_source ts ON ts.source_id=s.source_id JOIN track t ON t.id=ts.track_id JOIN library_membership lm ON lm.track_id=t.id ORDER BY t.release_id,t.disc_number,t.track_number,t.id")?.query_map([], |r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))? {
            let (release,track)=row?; releases.entry(release).or_default().push(TrackId(track));
        }
        report.releases = releases
            .into_iter()
            .map(|(id, track_ids)| ImportedRelease {
                release_id: ReleaseId(id),
                track_ids,
            })
            .collect();
        progress(Progress {
            scanned: report.scanned,
            imported: report.imported,
        });
        Ok(report)
    }
    fn ingest_root(
        &mut self,
        root: &RootId,
        explicit: bool,
        extractor: &mut dyn MetadataExtractor,
        report: &mut Report,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<()> {
        let path = self.store.root_path(root)?;
        let scan = self.store.begin_scan(root)?;
        let result = (|| -> Result<bool> {
            let mut complete = true;
            let mut batch = Vec::with_capacity(BATCH);
            for entry in WalkDir::new(&path).follow_links(false) {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => {
                        report.unreadable += 1;
                        complete = false;
                        continue;
                    }
                };
                if !entry.file_type().is_file() {
                    continue;
                }
                if !extractor.supports(entry.path()) {
                    report.unsupported += 1;
                    continue;
                }
                batch.push(entry.path().to_path_buf());
                if batch.len() == BATCH {
                    self.ingest_batch(
                        &batch,
                        Some(root),
                        Some(scan),
                        explicit,
                        false,
                        extractor,
                        report,
                    )?;
                    batch.clear();
                    progress(Progress {
                        scanned: report.scanned,
                        imported: report.imported,
                    });
                }
            }
            self.ingest_batch(
                &batch,
                Some(root),
                Some(scan),
                explicit,
                false,
                extractor,
                report,
            )?;
            Ok(complete)
        })();
        match result {
            Ok(true) => {
                self.store.complete_scan(root, scan)?;
            }
            Ok(false) => {
                self.store.fail_scan(scan)?;
                report.locations_failed += 1;
            }
            Err(e) => {
                self.store.fail_scan(scan)?;
                return Err(e);
            }
        }
        Ok(())
    }
    fn known_ingestion_sources(&self, paths: &[PathBuf]) -> Result<HashMap<PathBuf, Vec<Known>>> {
        let paths = serde_json::to_string(
            &paths
                .iter()
                .map(|p| hex(&path_to_bytes(p)))
                .collect::<Vec<_>>(),
        )
        .expect("paths");
        let mut known = HashMap::<PathBuf, Vec<Known>>::new();
        let mut stmt = self.store.connection.prepare("SELECT l.path,l.root_id,l.source_id,l.size_bytes,l.modified_ns,x.source_id IS NOT NULL,COALESCE(m.genres_observed,0)
            FROM json_each(?1) p CROSS JOIN local_file_observation l INDEXED BY local_file_path ON l.path=unhex(p.value)
            LEFT JOIN local_source_suppression x ON x.source_id=l.source_id LEFT JOIN file_metadata_observation m ON m.source_id=l.source_id")?;
        for row in stmt.query_map([paths], |r| {
            Ok((
                bytes_to_path(r.get(0)?),
                Known {
                    root: r.get(1)?,
                    observation: KnownLocalSource {
                        source_id: SourceId(r.get(2)?),
                        size_bytes: r.get::<_, i64>(3)? as u64,
                        modified_ns: r.get(4)?,
                        genres_observed: r.get(6)?,
                    },
                    suppressed: r.get(5)?,
                },
            ))
        })? {
            let (path, item) = row?;
            known.entry(path).or_default().push(item);
        }
        Ok(known)
    }
    #[allow(clippy::too_many_arguments)]
    fn ingest_batch(
        &mut self,
        paths: &[PathBuf],
        root: Option<&RootId>,
        scan: Option<i64>,
        explicit: bool,
        file_selection: bool,
        extractor: &mut dyn MetadataExtractor,
        report: &mut Report,
    ) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let known = self.known_ingestion_sources(paths)?;
        let mut items = Vec::with_capacity(paths.len());
        let mut admitted = Vec::with_capacity(paths.len());
        let mut seen = Vec::with_capacity(paths.len());
        for path in paths {
            let candidates: Vec<_> = known.get(path).into_iter().flatten().collect();
            // Never merge ambiguous pre-existing sources from overlapping roots.
            let candidates: Vec<_> = if !file_selection
                && candidates
                    .iter()
                    .any(|k| k.root.as_deref() == root.map(AsRef::as_ref))
            {
                candidates
                    .into_iter()
                    .filter(|k| k.root.as_deref() == root.map(AsRef::as_ref))
                    .collect()
            } else {
                candidates
            };
            if candidates.len() > 1 {
                seen.extend(candidates.iter().map(|k| k.observation.source_id.clone()));
                report.unreadable += 1;
                continue;
            }
            let known = candidates.first().copied();
            report.scanned += 1;
            if let Some(k) = known.filter(|k| !explicit && k.suppressed) {
                // Observe presence without reparsing excluded audio or admitting membership.
                items.push(crate::storage::ScannedLocalSource {
                    source_id: Some(k.observation.source_id.clone()),
                    path: path.clone(),
                    size_bytes: k.observation.size_bytes,
                    modified_ns: k.observation.modified_ns,
                    metadata: None,
                });
                continue;
            }
            match observe(path, known.map(|k| &k.observation), extractor) {
                Ok(mut item) => {
                    if item.metadata.is_some() {
                        report.parsed += 1;
                    } else {
                        report.unchanged += 1;
                    }
                    let id = item.source_id.get_or_insert_with(SourceId::new).clone();
                    admitted.push(id);
                    items.push(item);
                }
                Err(_) => {
                    if let Some(k) = known {
                        seen.push(k.observation.source_id.clone());
                    }
                    report.unreadable += 1;
                }
            }
        }
        // Encountered malformed/suppressed known files are not mistaken for missing files.
        if let Some(scan) = scan.filter(|_| !seen.is_empty()) {
            let json = source_json(&seen);
            let tx = self.store.connection.transaction()?;
            tx.execute("UPDATE local_file_observation SET last_seen_scan_id=?1 WHERE source_id IN (SELECT value FROM json_each(?2))", params![scan,&json])?;
            tx.execute("INSERT INTO local_root_source(root_id,source_id,last_seen_scan_id) SELECT ?1,value,?2 FROM json_each(?3) WHERE 1
                ON CONFLICT(root_id,source_id) DO UPDATE SET last_seen_scan_id=excluded.last_seen_scan_id",params![root.map(AsRef::as_ref),scan,&json])?;
            tx.commit()?;
        }
        self.store.apply_local_batch(root, scan, &items)?;
        let json = source_json(&admitted);
        let tx = self.store.connection.transaction()?;
        if explicit {
            // Unassociated exclusions are cleared only inside successful import_release.
            tx.execute("DELETE FROM local_source_suppression WHERE source_id IN (SELECT j.value FROM json_each(?1) j JOIN track_source ts ON ts.source_id=j.value)", [&json])?;
        }
        report.imported += tx.execute("INSERT OR IGNORE INTO library_membership(track_id) SELECT ts.track_id FROM json_each(?1) j CROSS JOIN track_source ts ON ts.source_id=j.value JOIN local_file_observation l ON l.source_id=ts.source_id WHERE l.available=1 AND NOT EXISTS(SELECT 1 FROM local_source_suppression x WHERE x.source_id=l.source_id)", [&json])? as u64;
        tx.execute(
            "INSERT OR IGNORE INTO ingestion_sources SELECT value FROM json_each(?1)",
            [&json],
        )?;
        tx.commit()?;
        let candidates =
            self.store
                .local_candidates(None, admitted.len() as u32, Some(&admitted), explicit)?;
        let tx = self.store.connection.transaction()?;
        for c in candidates {
            let title = c
                .metadata
                .release_title
                .as_deref()
                .unwrap_or("Unknown Album");
            let artists = if c.metadata.release_artists.is_empty() {
                &c.metadata.track_artists
            } else {
                &c.metadata.release_artists
            };
            tx.execute(
                "INSERT OR IGNORE INTO ingestion_candidates VALUES (?1,?2,?3,?4)",
                params![
                    c.source_id.as_ref(),
                    path_to_bytes(c.path.parent().unwrap_or(Path::new(""))),
                    crate::matching::normalize(title),
                    crate::matching::credit(artists).unwrap_or_default()
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    fn import_candidate_groups(
        &mut self,
        report: &mut Report,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<()> {
        loop {
            let group = self.store.connection.query_row("SELECT parent,title_key,credit_key FROM ingestion_candidates ORDER BY parent,title_key,credit_key LIMIT 1", [], |r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).optional()?;
            let Some((parent, title, credit)) = group else {
                break;
            };
            let ids: Vec<_> = self.store.connection.prepare("SELECT source_id FROM ingestion_candidates WHERE parent=?1 AND title_key=?2 AND credit_key=?3 ORDER BY source_id")?.query_map(params![&parent,&title,&credit], |r|r.get::<_,String>(0).map(SourceId))?.collect::<rusqlite::Result<_>>()?;
            // Same grouping, observations, provenance, and reconciliation as scanner import.
            let candidates =
                self.store
                    .local_candidates(None, ids.len() as u32, Some(&ids), true)?;
            if let Some(first) = candidates.first() {
                let request = ImportReleaseRequest {
                    release_title: first
                        .metadata
                        .release_title
                        .clone()
                        .unwrap_or_else(|| "Unknown Album".into()),
                    release_artists: vec![],
                    tracks: candidates
                        .iter()
                        .map(|c| ImportTrackInput {
                            source_id: c.source_id.clone(),
                            title_fallback: Some(
                                c.path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into(),
                            ),
                            artists: vec![],
                            disc_number: c.metadata.disc_number,
                            track_number: c.metadata.track_number,
                        })
                        .collect(),
                };
                match self.import_release(&request) {
                    Ok(imported) => report.imported += imported.track_ids.len() as u64,
                    Err(crate::Error::Database(error)) => return Err(error.into()),
                    Err(_) => report.unreadable += request.tracks.len() as u64,
                }
            }
            self.store.connection.execute("DELETE FROM ingestion_candidates WHERE parent=?1 AND title_key=?2 AND credit_key=?3", params![parent,title,credit])?;
            progress(Progress {
                scanned: report.scanned,
                imported: report.imported,
            });
        }
        Ok(())
    }
}
/// Background ingestion owns its own connection and performs no provider requests.
pub struct Worker(PathBuf);
impl Worker {
    pub fn run(self, request: Request, progress: &mut dyn FnMut(Progress)) -> Result<Report> {
        Library::open(self.0)?.ingest_local(
            &request,
            &mut crate::filesystem::LoftyMetadataExtractor,
            progress,
        )
    }
}
