//! Durable reconciliation lifecycle. This module performs no provider requests.
use crate::{
    Library,
    catalog::Page,
    domain::{AlbumId, TrackId},
    song_resolution::{Candidate, Input},
    storage::{Error, Result},
};
use rusqlite::params;

/// Increment when acceptance/evidence semantics change. Successful identities are never invalidated.
pub const EVALUATION_VERSION: u32 = 5;
pub const LOCAL_BATCH: u32 = 32;
pub const RETRY_ALBUMS: u32 = 20;
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct LocalBatch {
    pub examined: usize,
    pub updated: Vec<TrackId>,
    pub accepted: Vec<TrackId>,
    pub needs_retry: usize,
    pub still_unresolved: usize,
    pub errors: Vec<String>,
}
fn json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| Error::Invalid(e.to_string()))
}
pub(crate) fn excluded(db: &rusqlite::Connection, track: &TrackId, provider: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM track_provider_exclusion WHERE track_id=?1 AND provider=?2)",
        params![track.as_ref(), provider],
        |r| r.get(0),
    )?)
}
pub(crate) fn album_unexcluded(db: &rusqlite::Connection, album: &AlbumId) -> Result<bool> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM track t JOIN release r ON r.id=t.release_id WHERE r.album_id=?1 AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify'))",[album.as_ref()],|r|r.get(0))?)
}
impl Library {
    /// Separate write connection for bounded local work away from the UI thread.
    pub fn spotify_review_worker(&self) -> Result<Option<Self>> {
        self.store
            .connection
            .path()
            .filter(|p| !p.is_empty())
            .map(Self::open)
            .transpose()
    }
    pub fn spotify_manually_excluded(&self, track: &TrackId) -> Result<bool> {
        excluded(&self.store.connection, track, "spotify")
    }
    pub fn mark_not_on_spotify(&mut self, track: &TrackId) -> Result<()> {
        self.store.connection.execute("INSERT INTO track_provider_exclusion(track_id,provider) VALUES(?1,'spotify') ON CONFLICT DO NOTHING",[track.as_ref()])?;
        Ok(())
    }
    /// One atomic user action. Connections established since selection are retained.
    pub fn mark_tracks_not_on_spotify(&mut self, tracks: &[TrackId]) -> Result<usize> {
        let tx = self.store.connection.transaction()?;
        let changed = tx.execute(
            "INSERT INTO track_provider_exclusion(track_id,provider) SELECT t.id,'spotify' FROM track t WHERE t.id IN(SELECT value FROM json_each(?1)) AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id) ON CONFLICT DO NOTHING",
            [json(&tracks)?],
        )?;
        tx.commit()?;
        Ok(changed)
    }
    /// Re-enable eligibility; clearing negative knowledge never creates an identity.
    pub fn check_spotify_again(&mut self, track: &TrackId) -> Result<()> {
        let tx = self.store.connection.transaction()?;
        tx.execute(
            "DELETE FROM track_provider_exclusion WHERE track_id=?1 AND provider='spotify'",
            [track.as_ref()],
        )?;
        tx.execute(
            "UPDATE spotify_connection_review SET stale=1 WHERE track_id=?1",
            [track.as_ref()],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn marked_spotify_count(&self) -> Result<u64> {
        Ok(self.store.connection.query_row("SELECT count(*) FROM track_provider_exclusion x INDEXED BY provider_exclusion_list JOIN library_membership m ON m.track_id=x.track_id WHERE x.provider='spotify'",[],|r|r.get::<_,i64>(0))? as u64)
    }
    /// Bound the explicit retry plan and deduplicate by Album. Display filters do not change eligibility.
    pub fn spotify_retry_albums(
        &self,
        after: Option<&AlbumId>,
        limit: u32,
    ) -> Result<Vec<AlbumId>> {
        let mut q=self.store.connection.prepare("SELECT b.album_id FROM album_browse_order b INDEXED BY album_order_title WHERE b.title_key>=COALESCE((SELECT title_key FROM album_browse_order WHERE album_id=?1),'') AND (?1 IS NULL OR (b.title_key,b.album_id)>(SELECT title_key,album_id FROM album_browse_order WHERE album_id=?1)) AND EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id=b.album_id AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id)) ORDER BY b.title_key,b.album_id LIMIT ?2")?;
        Ok(q.query_map(
            params![after.map(AsRef::as_ref), limit.clamp(1, RETRY_ALBUMS)],
            |r| Ok(AlbumId(r.get(0)?)),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Bounded Track fallback after a batch's Album work; no provider activity here.
    pub fn spotify_retry_tracks(&self, albums: &[AlbumId], limit: u32) -> Result<Vec<TrackId>> {
        self.spotify_retry_tracks_after(albums, None, limit)
    }
    /// Cursor through the complete fallback set; the page bound is not a total-work cap.
    pub fn spotify_retry_tracks_after(
        &self,
        albums: &[AlbumId],
        after: Option<&TrackId>,
        limit: u32,
    ) -> Result<Vec<TrackId>> {
        let mut q=self.store.connection.prepare("SELECT t.id FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id IN(SELECT value FROM json_each(?1)) AND (?3 IS NULL OR (r.album_id,COALESCE(t.disc_number,1),COALESCE(t.track_number,2147483647),t.id)>(SELECT ar.album_id,COALESCE(at.disc_number,1),COALESCE(at.track_number,2147483647),at.id FROM track at JOIN release ar ON ar.id=at.release_id WHERE at.id=?3)) AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id) ORDER BY r.album_id,COALESCE(t.disc_number,1),COALESCE(t.track_number,2147483647),t.id LIMIT ?2")?;
        Ok(q.query_map(
            params![json(&albums)?, limit.clamp(1, 20), after.map(AsRef::as_ref)],
            |r| Ok(TrackId(r.get(0)?)),
        )?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn spotify_album_has_eligible(&self, album: &AlbumId) -> Result<bool> {
        Ok(self.store.connection.query_row("SELECT EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id=?1 AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id))",[album.as_ref()],|r|r.get(0))?)
    }
    /// A complete bounded page retains competition evidence, not merely its chosen candidate.
    pub(crate) fn cache_spotify_attempt(
        &mut self,
        input: &Input,
        page: &Page<Candidate>,
    ) -> Result<()> {
        if input.spotify_excluded || page.items.iter().any(|c| c.identity.provider != "spotify") {
            return Ok(());
        }
        let tx = self.store.connection.transaction()?;
        let current = crate::song_resolution::load_input(&tx, &input.track_id)?;
        if current.spotify_excluded {
            return Ok(());
        }
        if page.items.len() <= 10 {
            tx.execute("INSERT INTO spotify_reconciliation_cache(track_id,input_json,page_json) VALUES(?1,?2,?3) ON CONFLICT(track_id) DO UPDATE SET input_json=excluded.input_json,page_json=excluded.page_json",params![input.track_id.as_ref(),json(input)?,json(page)?])?;
        } else {
            tx.execute(
                "DELETE FROM spotify_reconciliation_cache WHERE track_id=?1",
                [input.track_id.as_ref()],
            )?;
        }
        tx.execute("UPDATE spotify_connection_review SET evaluation_version=?2,stale=?3,state='unresolved' WHERE track_id=?1",params![input.track_id.as_ref(),EVALUATION_VERSION,input!=&current])?;
        tx.commit()?;
        Ok(())
    }
    /// Indexed, cursor-bounded stale scan. Exclusions/trusted identities are filtered in SQL.
    pub fn stale_spotify_tracks(&self, limit: u32) -> Result<Vec<TrackId>> {
        let eligible = " AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=s.track_id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track t WHERE t.track_id=s.track_id)";
        let sql = format!(
            "SELECT track_id FROM (SELECT s.track_id FROM spotify_connection_review s INDEXED BY spotify_review_stale JOIN library_membership m ON m.track_id=s.track_id WHERE s.stale=1 {eligible} LIMIT ?2) UNION SELECT track_id FROM (SELECT s.track_id FROM spotify_connection_review s INDEXED BY spotify_review_version JOIN library_membership m ON m.track_id=s.track_id WHERE s.evaluation_version<?1 {eligible} LIMIT ?2) LIMIT ?2"
        );
        let mut q = self.store.connection.prepare(&sql)?;
        Ok(q.query_map(
            params![EVALUATION_VERSION, limit.clamp(1, LOCAL_BATCH)],
            |r| Ok(TrackId(r.get(0)?)),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Network-free work unit. Call on a worker connection, delivering targeted updates between batches.
    pub fn reevaluate_stale_spotify(&mut self, limit: u32) -> Result<LocalBatch> {
        let ids = self.stale_spotify_tracks(limit)?;
        let mut batch = LocalBatch::default();
        let ids_json = json(&ids)?;
        // Identify replayable rows in one batch; legacy reasons alone contain no candidate data.
        let replayable:std::collections::HashSet<String>=self.store.connection.prepare("SELECT c.track_id FROM spotify_reconciliation_cache c WHERE c.track_id IN(SELECT value FROM json_each(?1)) UNION SELECT t.id FROM spotify_program_cache c JOIN release r ON r.album_id=c.album_id JOIN track t ON t.release_id=r.id WHERE t.id IN(SELECT value FROM json_each(?1))")?.query_map([&ids_json],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        let cached:std::collections::HashMap<TrackId,(String,String)>=self.store.connection.prepare("SELECT track_id,input_json,page_json FROM spotify_reconciliation_cache WHERE track_id IN(SELECT value FROM json_each(?1))")?.query_map([&ids_json],|r|Ok((TrackId(r.get(0)?),(r.get(1)?,r.get(2)?))))?.collect::<rusqlite::Result<_>>()?;
        let programs:std::collections::HashMap<AlbumId,String>=self.store.connection.prepare("SELECT album_id,programs_json FROM spotify_program_cache WHERE album_id IN(SELECT DISTINCT r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id IN(SELECT value FROM json_each(?1)))")?.query_map([&ids_json],|r|Ok((AlbumId(r.get(0)?),r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        let missing: Vec<_> = ids
            .iter()
            .filter(|t| !replayable.contains(t.as_ref()))
            .collect();
        if !missing.is_empty() {
            let changed=self.store.connection.execute("UPDATE spotify_connection_review SET evaluation_version=?2,stale=0,state='needs_retry' WHERE track_id IN(SELECT value FROM json_each(?1)) AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=spotify_connection_review.track_id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=spotify_connection_review.track_id)",params![json(&missing)?,EVALUATION_VERSION])?;
            batch.updated.extend(missing.into_iter().cloned());
            batch.examined += changed;
            batch.needs_retry += changed;
        }
        for track in ids.into_iter().filter(|t| replayable.contains(t.as_ref())) {
            let pending:bool=self.store.connection.query_row("SELECT (stale=1 OR evaluation_version<?2) AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=?1 AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=?1) FROM spotify_connection_review WHERE track_id=?1",params![track.as_ref(),EVALUATION_VERSION],|r|r.get(0))?;
            if !pending {
                continue;
            }
            batch.updated.push(track.clone());
            batch.examined += 1;
            if let Err(error) =
                self.reevaluate_one(&track, cached.get(&track), &programs, &mut batch)
            {
                batch.errors.push(format!("{}: {error}", track.as_ref()));
                // Do not spin on an unavailable database/cache; explicit retry remains possible.
                self.store.connection.execute("UPDATE spotify_connection_review SET evaluation_version=?2,stale=0,state='needs_retry' WHERE track_id=?1",params![track.as_ref(),EVALUATION_VERSION])?;
            }
        }
        Ok(batch)
    }
    fn reevaluate_one(
        &mut self,
        track: &TrackId,
        cached: Option<&(String, String)>,
        programs: &std::collections::HashMap<AlbumId, String>,
        batch: &mut LocalBatch,
    ) -> Result<()> {
        let mut song_evaluated = false;
        if let Some((snapshot, page)) = cached {
            let input = self.song_resolution_input(track)?;
            if input.spotify_excluded {
                return Ok(());
            }
            let snapshot: Input =
                serde_json::from_str(snapshot).map_err(|e| Error::Invalid(e.to_string()))?;
            let page: Page<Candidate> =
                serde_json::from_str(page).map_err(|e| Error::Invalid(e.to_string()))?;
            // A new discovery scope needs a fresh complete page. Evidence within the same
            // scope (date, type, positions, trusted IDs/equivalence) may be reconsidered locally.
            if !page.items.is_empty()
                && page.next_offset.is_none()
                && crate::matching::normalize_album_title(&snapshot.title)
                    == crate::matching::normalize_album_title(&input.title)
                && crate::matching::normalize_album_title(&snapshot.album)
                    == crate::matching::normalize_album_title(&input.album)
                && snapshot.primary_artist.as_ref().map(|a| &a.name)
                    == input.primary_artist.as_ref().map(|a| &a.name)
            {
                let accepted = self.apply_song_evaluation(&input, &page)?;
                self.persist_spotify_song_review(&input, &page)?;
                if accepted.is_some() {
                    batch.accepted.push(track.clone());
                    return Ok(());
                }
                // A failed Track search must not suppress stronger accumulated
                // Album-program evidence already persisted for this context.
                song_evaluated = true;
            }
        }
        // A persisted program can be replayed only under its still-established Album identity.
        let album = self.album_id_for_track(track)?;
        if let Some(program) = programs.get(&album) {
            let program: crate::album_program::Programs =
                serde_json::from_str(program).map_err(|e| Error::Invalid(e.to_string()))?;
            if let Some(current) = self.prepare_album_program(&album, &program.album)? {
                let eligible:Vec<TrackId>=self.store.connection.prepare("SELECT t.id FROM track t JOIN release r ON r.id=t.release_id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id=?1 AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id)")?.query_map([album.as_ref()],|r|Ok(TrackId(r.get(0)?)))?.collect::<rusqlite::Result<_>>()?;
                let outcome = self.complete_album_program(crate::album_program::Reply {
                    input: current,
                    result: Ok(program),
                })?;
                match outcome {
                    crate::album_program::Outcome::Error(e) => return Err(Error::Invalid(e)),
                    crate::album_program::Outcome::Deferred(e) => {
                        return Err(Error::Invalid(e.to_string()));
                    }
                    _ => {}
                }
                let accepted:Vec<TrackId>=self.store.connection.prepare("SELECT DISTINCT s.track_id FROM trusted_spotify_track s WHERE s.track_id IN(SELECT value FROM json_each(?1))")?.query_map([json(&eligible)?],|r|Ok(TrackId(r.get(0)?)))?.collect::<rusqlite::Result<_>>()?;
                batch.examined += eligible.len().saturating_sub(1);
                batch.still_unresolved += eligible.len().saturating_sub(accepted.len());
                batch.updated.extend(eligible);
                batch.accepted.extend(accepted);
                return Ok(());
            }
        }
        if song_evaluated {
            batch.still_unresolved += 1;
            return Ok(());
        }
        self.store.connection.execute("UPDATE spotify_connection_review SET state='needs_retry',evaluation_version=?2,stale=0 WHERE track_id=?1",params![track.as_ref(),EVALUATION_VERSION])?;
        batch.needs_retry += 1;
        Ok(())
    }
}
