//! Complete provider snapshots. No networking, saved membership or queue mutation.
use crate::{
    Library, Result,
    catalog::{Credit, Duration},
    domain::*,
    storage::{self, Error},
};
use rusqlite::{OptionalExtension, params};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct Item {
    pub identity: ExternalIdentity,
    pub title: String,
    pub credits: Vec<Credit>,
    pub release_identity: ExternalIdentity,
    pub release_title: String,
    pub release_credits: Vec<Credit>,
    pub year: Option<i32>,
    pub disc: Option<u32>,
    pub number: Option<u32>,
    pub duration: Option<Duration>,
}
#[derive(Clone, Debug)]
pub struct Plan {
    pub provider: String,
    pub external_id: String,
    pub source_url: String,
    pub version: Option<String>,
    pub owner: String,
    pub name: String,
    pub items: Vec<Item>,
    pub unsupported: usize,
    pub unavailable: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Imported { playlist_id: String, entries: usize },
    AlreadyImported { playlist_id: String },
}
/// Explicit snapshot decisions; titles never serve as durable identity.
#[derive(Clone, Debug)]
pub enum Decision {
    Create,
    Rename(String),
    Overwrite(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub playlist_id: String,
    pub name: String,
    pub same_source: bool,
}
impl Library {
    pub fn playlist_import_path(&self) -> Result<std::path::PathBuf> {
        self.store
            .connection
            .path()
            .filter(|p| !p.is_empty())
            .map(Into::into)
            .ok_or_else(|| Error::Invalid("Playlist import requires a file-backed library".into()))
    }
    pub fn imported_playlist(&self, provider: &str, id: &str) -> Result<Option<String>> {
        Ok(self
            .store
            .connection
            .query_row(
                "SELECT playlist_id FROM playlist_source WHERE provider=?1 AND external_id=?2",
                params![provider, id],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub fn playlist_import_conflicts(&self, plan: &Plan) -> Result<Vec<Conflict>> {
        conflicts(&self.store.connection, plan)
    }
    /// Persist an explicitly resolved, completely fetched snapshot. No remote work here.
    pub fn resolve_playlist_import(&mut self, plan: &Plan, decision: &Decision) -> Result<Outcome> {
        self.persist_playlist_snapshot(plan, Some(decision))
    }
    /// All remote pages must be fetched before calling. Every durable write rolls back together.
    pub fn import_playlist_snapshot(&mut self, plan: &Plan) -> Result<Outcome> {
        self.persist_playlist_snapshot(plan, None)
    }
    fn persist_playlist_snapshot(
        &mut self,
        plan: &Plan,
        decision: Option<&Decision>,
    ) -> Result<Outcome> {
        if plan.provider.is_empty()
            || plan.external_id.is_empty()
            || plan.name.trim().is_empty()
            || plan.items.iter().any(|i| {
                i.identity.provider != plan.provider
                    || i.identity.kind != "track"
                    || i.identity.external_id.is_empty()
                    || i.release_identity.provider != plan.provider
                    || i.release_identity.kind.is_empty()
                    || i.release_identity.external_id.is_empty()
            })
        {
            return Err(Error::Invalid("Invalid playlist import plan".into()));
        }
        let tx = self
            .store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if decision.is_none()
            && let Some(id) = tx
                .query_row(
                    "SELECT playlist_id FROM playlist_source WHERE provider=?1 AND external_id=?2",
                    params![plan.provider, plan.external_id],
                    |r| r.get(0),
                )
                .optional()?
        {
            return Ok(Outcome::AlreadyImported { playlist_id: id });
        }
        let mut tracks = trusted_tracks(
            &tx,
            &plan.provider,
            &plan
                .items
                .iter()
                .map(|i| i.identity.external_id.clone())
                .collect::<Vec<_>>(),
            true,
        )?;
        let collisions = conflicts(&tx, plan)?;
        let playlist = match decision {
            Some(Decision::Overwrite(id)) => {
                // An ambiguous source/title collision must be resolved by renaming.
                if collisions.len() != 1 || collisions[0].playlist_id != *id {
                    return Err(Error::Invalid(
                        "Choose Rename incoming: the overwrite target is ambiguous or changed."
                            .into(),
                    ));
                }
                tx.execute("DELETE FROM playlist_entry WHERE playlist_id=?1", [id])?;
                tx.execute("DELETE FROM playlist_source WHERE playlist_id=?1", [id])?;
                id.clone()
            }
            _ => {
                let name = match decision {
                    Some(Decision::Rename(name)) => name.trim(),
                    _ => plan.name.trim(),
                };
                if name.is_empty() || name.chars().any(char::is_control) {
                    return Err(Error::Invalid(
                        "Enter a non-empty usable playlist title.".into(),
                    ));
                }
                let title_exists: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM playlist WHERE name=?1 COLLATE NOCASE)",
                    [name],
                    |r| r.get(0),
                )?;
                if (title_exists && decision.is_some())
                    || (matches!(decision, Some(Decision::Create)) && !collisions.is_empty())
                {
                    return Err(Error::Invalid("A playlist with this title or Spotify source already exists. Choose Rename incoming or Overwrite existing.".into()));
                }
                let id = uuid::Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT INTO playlist(id,name) VALUES(?1,?2)",
                    params![id, name],
                )?;
                id
            }
        };
        tx.execute(
            "INSERT INTO playlist_source VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                playlist,
                plan.provider,
                plan.external_id,
                plan.source_url,
                plan.version,
                plan.owner
            ],
        )?;
        let mut releases: HashMap<(String, String), ReleaseId> = HashMap::new();
        let mut durations = HashSet::new();
        for (position, item) in plan.items.iter().enumerate() {
            let track = if let Some(track) = tracks.get(&item.identity.external_id) {
                track.clone()
            } else {
                let rid = &item.release_identity;
                let release_key = (rid.kind.clone(), rid.external_id.clone());
                let release = if let Some(release) = releases.get(&release_key) {
                    release.clone()
                } else {
                    let ids = {
                        let mut q=tx.prepare("SELECT release_id FROM release_external_identity WHERE provider=?1 AND kind=?2 AND external_id=?3")?;
                        q.query_map(params![rid.provider, rid.kind, rid.external_id], |r| {
                            r.get::<_, String>(0)
                        })?
                        .collect::<std::result::Result<Vec<_>, _>>()?
                    };
                    if ids.len() > 1 {
                        return Err(Error::Invalid("Ambiguous playlist Release identity".into()));
                    }
                    let release = if let Some(id) = ids.first() {
                        ReleaseId(id.clone())
                    } else {
                        let album =
                            storage::create_album_tx(&tx, &item.release_title, item.year, &[])?;
                        storage::insert_catalog_credits(
                            &tx,
                            "album_artist_credit",
                            album.as_ref(),
                            &item.release_credits,
                        )?;
                        storage::refresh_album_match_key(&tx, album.as_ref())?;
                        let release = ReleaseId::new();
                        tx.execute(
                            "INSERT INTO release(id,album_id) VALUES(?1,?2)",
                            params![release.as_ref(), album.as_ref()],
                        )?;
                        tx.execute("INSERT INTO release_application_metadata(release_id,title,year) VALUES(?1,?2,?3)",params![release.as_ref(),item.release_title,item.year])?;
                        storage::insert_catalog_credits(
                            &tx,
                            "release_artist_credit",
                            release.as_ref(),
                            &item.release_credits,
                        )?;
                        tx.execute(
                            "INSERT INTO release_external_identity VALUES(?1,?2,?3,?4)",
                            params![release.as_ref(), rid.provider, rid.kind, rid.external_id],
                        )?;
                        tx.execute(
                            "INSERT INTO album_external_identity VALUES(?1,?2,?3,?4)",
                            params![album.as_ref(), rid.provider, "album", rid.external_id],
                        )?;
                        release
                    };
                    releases.insert(release_key, release.clone());
                    release
                };
                let track = TrackId::new();
                let recording = crate::recording::create_recording_tx(&tx)?;
                tx.execute("INSERT INTO track(id,release_id,disc_number,track_number,recording_id) VALUES(?1,?2,?3,?4,?5)",params![track.as_ref(),release.as_ref(),item.disc,item.number,recording.as_ref()])?;
                tx.execute(
                    "INSERT INTO track_application_metadata(track_id,title) VALUES(?1,?2)",
                    params![track.as_ref(), item.title],
                )?;
                storage::insert_catalog_credits(
                    &tx,
                    "track_artist_credit",
                    track.as_ref(),
                    &item.credits,
                )?;
                tx.execute(
                    "INSERT INTO track_external_identity VALUES(?1,?2,?3,?4)",
                    params![
                        track.as_ref(),
                        item.identity.provider,
                        item.identity.kind,
                        item.identity.external_id
                    ],
                )?;
                storage::refresh_effective_track_tx(&tx, &track)?;

                tracks.insert(item.identity.external_id.clone(), track.clone());
                track
            };
            if durations.insert(track.clone())
                && let Some(duration) = item.duration
            {
                let milliseconds = i64::try_from(duration.milliseconds)
                    .map_err(|_| Error::Invalid("Invalid duration".into()))?;
                tx.execute("INSERT INTO track_duration_observation VALUES(?1,?2,?3,?4,?5) ON CONFLICT DO NOTHING",params![track.as_ref(),plan.provider,item.identity.external_id,milliseconds,if duration.approximate {2} else {0}])?;
                storage::refresh_effective_track_tx(&tx, &track)?;
            }
            tx.execute(
                "INSERT INTO playlist_entry(id,playlist_id,track_id,position) VALUES(?1,?2,?3,?4)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    playlist,
                    track.as_ref(),
                    position as i64
                ],
            )?;
        }
        tx.execute("DELETE FROM playlist_import_identity", [])?;
        tx.commit()?;
        Ok(Outcome::Imported {
            playlist_id: playlist,
            entries: plan.items.len(),
        })
    }
}

fn conflicts(db: &rusqlite::Connection, plan: &Plan) -> Result<Vec<Conflict>> {
    let mut query=db.prepare("SELECT p.id,p.name,EXISTS(SELECT 1 FROM playlist_source s WHERE s.playlist_id=p.id AND s.provider=?1 AND s.external_id=?2) FROM playlist p WHERE p.name=?3 COLLATE NOCASE OR EXISTS(SELECT 1 FROM playlist_source s WHERE s.playlist_id=p.id AND s.provider=?1 AND s.external_id=?2) ORDER BY p.name COLLATE NOCASE,p.id")?;
    Ok(query
        .query_map(
            params![plan.provider, plan.external_id, plan.name.trim()],
            |r| {
                Ok(Conflict {
                    playlist_id: r.get(0)?,
                    name: r.get(1)?,
                    same_source: r.get(2)?,
                })
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// The same manual/direct/accepted-occurrence lookup is used by import and repair.
fn trusted_tracks(
    db: &rusqlite::Connection,
    provider: &str,
    identities: &[String],
    strict: bool,
) -> Result<HashMap<String, TrackId>> {
    db.execute_batch("CREATE TEMP TABLE IF NOT EXISTS playlist_import_identity(external_id TEXT PRIMARY KEY); DELETE FROM playlist_import_identity;")?;
    {
        let mut insert = db.prepare("INSERT OR IGNORE INTO playlist_import_identity VALUES(?1)")?;
        for id in identities {
            insert.execute([id])?;
        }
    }
    let sql = "SELECT i.external_id,i.track_id FROM track_external_identity i JOIN playlist_import_identity w ON w.external_id=i.external_id WHERE i.provider=?1 AND i.kind='track' AND NOT EXISTS(SELECT 1 FROM manual_track_association m WHERE m.track_id=i.track_id AND m.album_provider=?1)
        UNION SELECT json_extract(j.value,'$.external_id'),m.track_id FROM manual_track_association m,json_each(m.candidate_json,'$.evidence.identities') j JOIN playlist_import_identity w ON w.external_id=json_extract(j.value,'$.external_id') WHERE m.album_provider=?1 AND json_extract(j.value,'$.provider')=?1 AND json_extract(j.value,'$.kind')='track'
        UNION SELECT json_extract(j.value,'$.external_id'),a.track_id FROM provider_track_association a JOIN track t ON t.id=a.track_id JOIN release r ON r.id=t.release_id JOIN album_external_identity ai ON ai.album_id=r.album_id AND ai.provider=a.album_provider AND ai.kind=a.album_kind AND ai.external_id=a.album_external_id,json_each(a.match_json,'$.occurrences') j JOIN playlist_import_identity w ON w.external_id=json_extract(j.value,'$.external_id') WHERE a.album_provider=?1 AND json_extract(j.value,'$.provider')=?1 AND json_extract(j.value,'$.kind')='track' AND NOT EXISTS(SELECT 1 FROM manual_track_association m WHERE m.track_id=a.track_id AND m.album_provider=?1) AND NOT EXISTS(SELECT 1 FROM track_external_identity i WHERE i.track_id=a.track_id AND i.provider=?1)";
    let mut known: HashMap<String, HashSet<String>> = HashMap::new();
    let mut query = db.prepare(sql)?;
    for row in query.query_map([provider], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (id, track) = row?;
        known.entry(id).or_default().insert(track);
    }
    let candidate_ids: Vec<_> = known.values().flat_map(|ids| ids.iter().cloned()).collect();
    let local:HashSet<String>=db.prepare("SELECT DISTINCT track_id FROM track_source WHERE track_id IN (SELECT value FROM json_each(?1))")?.query_map([serde_json::to_string(&candidate_ids).expect("IDs")],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let mut result = HashMap::new();
    for (id, mut tracks) in known {
        let canonical: Vec<_> = tracks
            .iter()
            .filter(|t| local.contains(*t))
            .cloned()
            .collect();
        if canonical.len() == 1 {
            let target = &canonical[0];
            // Never override another saved/source-bearing Track's identity.
            let mut safe = true;
            for track in tracks.iter().filter(|t| *t != target) {
                let staged: bool=db.query_row("SELECT NOT EXISTS(SELECT 1 FROM library_membership WHERE track_id=?1) AND NOT EXISTS(SELECT 1 FROM track_source WHERE track_id=?1) AND EXISTS(SELECT 1 FROM track_external_identity WHERE track_id=?1 AND provider=?2 AND kind='track' AND external_id=?3)",params![track,provider,id],|r|r.get(0))?;
                safe &= staged;
            }
            if safe {
                tracks = HashSet::from([target.clone()]);
            }
        }
        if tracks.len() == 1 {
            result.insert(id, TrackId(tracks.into_iter().next().unwrap()));
        } else if strict {
            return Err(Error::Invalid(
                "Playlist Track identity has multiple associations; reconcile before importing"
                    .into(),
            ));
        }
        // Ambiguous trusted identities are left unresolved, never selected by row order.
    }
    Ok(result)
}

impl Library {
    /// Explicit, bounded reconciliation of persisted playlist Tracks. No network or matching.
    /// Only an already accepted provider identity can repoint an entry; duplicate entries survive.
    pub fn reconcile_playlist_tracks(&mut self, playlist: &str) -> Result<usize> {
        let mut after = -1;
        let mut changed = 0;
        loop {
            let (count, last) = self.reconcile_playlist_window(playlist, after)?;
            changed += count;
            let Some(last) = last else {
                return Ok(changed);
            };
            after = last;
        }
    }
    fn reconcile_playlist_window(
        &mut self,
        playlist: &str,
        after: i64,
    ) -> Result<(usize, Option<i64>)> {
        let tx = self
            .store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let rows: Vec<(String,String,String,i64)> = tx.prepare("SELECT p.id,p.track_id,i.external_id,p.position FROM playlist_entry p JOIN track_external_identity i ON i.track_id=p.track_id JOIN playlist_source s ON s.playlist_id=p.playlist_id AND s.provider=i.provider WHERE p.playlist_id=?1 AND p.position>?2 AND i.kind='track' AND NOT EXISTS(SELECT 1 FROM track_source WHERE track_id=p.track_id) AND NOT EXISTS(SELECT 1 FROM library_membership WHERE track_id=p.track_id) ORDER BY p.position LIMIT 200")?.query_map(params![playlist,after],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?.collect::<rusqlite::Result<_>>()?;
        let provider: Option<String> = tx
            .query_row(
                "SELECT provider FROM playlist_source WHERE playlist_id=?1",
                [playlist],
                |r| r.get(0),
            )
            .optional()?;
        let Some(provider) = provider else {
            return Ok((0, None));
        };
        let last = rows.last().map(|r| r.3);
        let known = trusted_tracks(
            &tx,
            &provider,
            &rows.iter().map(|r| r.2.clone()).collect::<Vec<_>>(),
            false,
        )?;
        let mut changed = 0;
        for (entry, old, identity, _) in rows {
            if let Some(target) = known.get(&identity)
                && target.as_ref() != old
            {
                changed += tx.execute(
                    "UPDATE playlist_entry SET track_id=?2 WHERE id=?1",
                    params![entry, target.as_ref()],
                )?;
            }
        }
        tx.commit()?;
        Ok((changed, last))
    }
    /// Names only discover bounded candidate Albums; existing provider matching decides identity.
    pub fn playlist_local_match_candidates(&self, playlist: &str) -> Result<Vec<ImportedRelease>> {
        let rows:Vec<(String,String)> = self.store.connection.prepare("SELECT DISTINCT m.match_title,m.match_artist_credit FROM playlist_entry p JOIN track t ON t.id=p.track_id JOIN release r ON r.id=t.release_id JOIN album_application_metadata m ON m.album_id=r.album_id WHERE p.playlist_id=?1 LIMIT 64")?.query_map([playlist],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        let mut releases = HashMap::<String, Vec<TrackId>>::new();
        let mut query=self.store.connection.prepare("SELECT t.release_id,t.id FROM album_application_metadata m JOIN release r ON r.album_id=m.album_id JOIN track t ON t.release_id=r.id WHERE m.match_title=?1 AND m.match_artist_credit=?2 AND EXISTS(SELECT 1 FROM track_source s WHERE s.track_id=t.id) LIMIT 201")?;
        for (title, artist) in rows {
            let candidates = query
                .query_map(params![title, artist], |r| {
                    Ok((r.get::<_, String>(0)?, TrackId(r.get(1)?)))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let unique: HashSet<_> = candidates.iter().map(|r| &r.0).collect();
            // A same-title local edition collision is not evidence for choosing one.
            if unique.len() != 1 || candidates.len() > 200 {
                continue;
            }
            for (release, track) in candidates {
                releases.entry(release).or_default().push(track);
            }
        }
        Ok(releases
            .into_iter()
            .map(|(id, track_ids)| ImportedRelease {
                release_id: ReleaseId(id),
                track_ids,
            })
            .collect())
    }
}

impl Library {
    /// Bounded program requests for the actual playlist occurrences, not a provider's
    /// alternate Album representation. Candidate discovery is not identity acceptance.
    pub fn playlist_reconciliation_programs(
        &self,
        playlist: &str,
    ) -> Result<Vec<(ReleaseId, ExternalIdentity)>> {
        let mut result = Vec::new();
        for local in self.playlist_local_match_candidates(playlist)? {
            let mut query=self.store.connection.prepare("SELECT DISTINCT i.provider,i.kind,i.external_id FROM track lt JOIN release lr ON lr.id=lt.release_id JOIN album_application_metadata lm ON lm.album_id=lr.album_id JOIN album_application_metadata pm ON pm.match_title=lm.match_title AND pm.match_artist_credit=lm.match_artist_credit JOIN release pr ON pr.album_id=pm.album_id JOIN release_external_identity i ON i.release_id=pr.id JOIN track pt ON pt.release_id=pr.id JOIN playlist_entry p ON p.track_id=pt.id JOIN playlist_source s ON s.playlist_id=p.playlist_id AND s.provider=i.provider WHERE lt.release_id=?1 AND p.playlist_id=?2 AND i.kind='album' AND NOT EXISTS(SELECT 1 FROM track_source WHERE track_id=pt.id) AND NOT EXISTS(SELECT 1 FROM library_membership WHERE track_id=pt.id) LIMIT 4")?;
            for row in query.query_map(params![local.release_id.as_ref(), playlist], |r| {
                Ok(ExternalIdentity {
                    provider: r.get(0)?,
                    kind: r.get(1)?,
                    external_id: r.get(2)?,
                })
            })? {
                result.push((local.release_id.clone(), row?));
            }
        }
        Ok(result)
    }
    /// Uses the existing full-program matcher, with additional trusted Artist/context gates.
    /// Does not accept an exact local edition, delete staging metadata, or alter membership.
    pub fn reconcile_playlist_album_program(
        &mut self,
        playlist: &str,
        release: &ReleaseId,
        programs: &crate::album_program::Programs,
    ) -> Result<usize> {
        use crate::album_program::{TrackOutcome, compare_album};
        if !self
            .playlist_reconciliation_programs(playlist)?
            .contains(&(release.clone(), programs.album.clone()))
        {
            return Ok(0);
        }
        let album = self.album_for_release(release)?;
        let local = self.local_album_tracks(&album.album_id)?;
        let comparisons = compare_album(&local, programs);
        let mut claims = HashMap::<String, usize>::new();
        for outcome in &comparisons {
            if let TrackOutcome::Matched(m) = outcome
                && let [id] = m.occurrences.as_slice()
            {
                *claims.entry(id.external_id.clone()).or_default() += 1;
            }
        }
        let tx = self
            .store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current = crate::album_program::local_tracks(&tx, &album.album_id)?;
        if current.0 != local {
            return Ok(0);
        }
        let still_referenced:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM playlist_entry p JOIN track t ON t.id=p.track_id JOIN release_external_identity i ON i.release_id=t.release_id WHERE p.playlist_id=?1 AND i.provider=?2 AND i.kind=?3 AND i.external_id=?4)",params![playlist,programs.album.provider,programs.album.kind,programs.album.external_id],|r|r.get(0))?;
        if !still_referenced {
            return Ok(0);
        }
        let mut associations = 0;
        for (local, outcome) in local.iter().zip(comparisons) {
            if current.1.contains(&local.track_id) {
                continue;
            }
            let has_source: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM track_source WHERE track_id=?1)",
                [local.track_id.as_ref()],
                |r| r.get(0),
            )?;
            if !has_source {
                continue;
            }
            let TrackOutcome::Matched(m) = outcome else {
                continue;
            };
            let [identity] = m.occurrences.as_slice() else {
                continue;
            };
            if claims.get(&identity.external_id) != Some(&1)
                || identity.provider != programs.album.provider
                || identity.kind != "track"
            {
                continue;
            }
            // A name agreement alone cannot establish this relationship. Require a
            // common trusted provider Artist in the supporting occurrence credits.
            let strong = programs
                .programs
                .iter()
                .flat_map(|p| &p.tracks)
                .filter(|t| t.identities.contains(identity))
                .all(|remote| {
                    let position = |a:Option<u32>,b:Option<u32>| !matches!((a,b),(Some(a),Some(b)) if a>0 && b>0 && a!=b);
                    position(local.evidence.disc,remote.disc) && position(local.evidence.number,remote.number) && local.evidence.artists.iter().any(|a| {
                        a.identities.iter().any(|id| {
                            id.provider == identity.provider
                                && remote.artists.iter().any(|b| b.identities.contains(id))
                        })
                    })
                });
            if !strong {
                continue;
            }
            let fixed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM track_external_identity WHERE track_id=?1 AND provider=?2) OR EXISTS(SELECT 1 FROM manual_track_association WHERE track_id=?1 AND album_provider=?2)",params![local.track_id.as_ref(),identity.provider],|r|r.get(0))?;
            if fixed {
                continue;
            }
            tx.execute("INSERT INTO track_external_identity(track_id,provider,kind,external_id) VALUES(?1,?2,?3,?4)",params![local.track_id.as_ref(),identity.provider,identity.kind,identity.external_id])?;
            if let Some(duration) = m.duration {
                storage::observe_duration(
                    &tx,
                    &local.track_id,
                    &identity.provider,
                    &identity.external_id,
                    duration,
                    if duration.approximate { 2 } else { 0 },
                )?;
            }
            associations += 1;
        }
        tx.commit()?;
        if associations == 0 {
            return Ok(0);
        }
        self.reconcile_playlist_tracks(playlist)
    }
}
