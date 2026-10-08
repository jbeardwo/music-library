//! Local-profile preferences. Canonical identity and metadata are never changed.
use crate::{
    Library, Result,
    browse::QueueReader,
    domain::{ArtistId, TrackId},
};
use rusqlite::params;
#[derive(Clone, Debug)]
pub enum IgnoreTarget {
    Track(String),
    Album(String),
    Artist(String),
}
// Same credit union as Artist browsing, including secondary Track credits.
fn tracks(target: &IgnoreTarget) -> (&str, &str) {
    match target {
        IgnoreTarget::Track(id) => ("SELECT id FROM track WHERE id=?1", id),
        IgnoreTarget::Album(id) => (
            "SELECT t.id FROM release r JOIN track t ON t.release_id=r.id WHERE r.album_id=?1",
            id,
        ),
        IgnoreTarget::Artist(id) => (
            "SELECT track_id id FROM track_artist_credit WHERE artist_id=?1 UNION SELECT t.id FROM album_artist_credit c JOIN release r ON r.album_id=c.album_id JOIN track t ON t.release_id=r.id WHERE c.artist_id=?1 UNION SELECT t.id FROM release_artist_credit c JOIN track t ON t.release_id=c.release_id WHERE c.artist_id=?1",
            id,
        ),
    }
}
impl Library {
    pub fn set_artist_hidden(&mut self, artist: &ArtistId, hidden: bool) -> Result<()> {
        let sql = if hidden {
            "INSERT OR IGNORE INTO hidden_artist_preference(profile_id,artist_id) VALUES('local',?1)"
        } else {
            "DELETE FROM hidden_artist_preference WHERE profile_id='local' AND artist_id=?1"
        };
        self.store.connection.execute(sql, [artist.as_ref()])?;
        Ok(())
    }
    pub fn artist_hidden(&self, artist: &ArtistId) -> Result<bool> {
        Ok(self.store.connection.query_row("SELECT EXISTS(SELECT 1 FROM hidden_artist_preference WHERE profile_id='local' AND artist_id=?1)",[artist.as_ref()],|r|r.get(0))?)
    }
    pub fn set_tracks_ignored(&mut self, target: &IgnoreTarget, ignored: bool) -> Result<()> {
        let (selection, id) = tracks(target);
        let tx = self.store.connection.transaction()?;
        let sql = if ignored {
            format!(
                "INSERT OR IGNORE INTO ignored_track_preference(profile_id,track_id) SELECT 'local',id FROM ({selection})"
            )
        } else {
            format!(
                "DELETE FROM ignored_track_preference WHERE profile_id='local' AND track_id IN ({selection})"
            )
        };
        tx.execute(&sql, [id])?;
        tx.commit()?;
        Ok(())
    }
    pub fn ignore_counts(&self, target: &IgnoreTarget) -> Result<(u64, u64)> {
        let (selection, id) = tracks(target);
        Ok(self.store.connection.query_row(&format!("SELECT count(*),count(p.track_id) FROM ({selection}) t LEFT JOIN ignored_track_preference p ON p.profile_id='local' AND p.track_id=t.id"),[id],|r|Ok((r.get::<_,i64>(0)? as u64,r.get::<_,i64>(1)? as u64)))?)
    }
    pub(crate) fn ignored_tracks_in(
        &self,
        tracks: &[&TrackId],
    ) -> Result<std::collections::HashSet<String>> {
        let ids = serde_json::to_string(&tracks.iter().map(|id| id.as_ref()).collect::<Vec<_>>())
            .expect("serialize Track IDs");
        let mut statement=self.store.connection.prepare("SELECT track_id FROM ignored_track_preference WHERE profile_id='local' AND track_id IN (SELECT value FROM json_each(?1))")?;
        Ok(statement
            .query_map([ids], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn is_track_eligible_for_generated_playback(&self, track: &TrackId) -> Result<bool> {
        eligible(&self.store.connection, track)
    }
    /// Bounded batches for presentation, including playlist rows. No per-row queries.
    pub fn ignored_in_targets(&self, targets: &[IgnoreTarget]) -> Result<Vec<bool>> {
        if targets.is_empty() {
            return Ok(vec![]);
        }
        // A scrolling window can contain 600 rows. Stay below SQLite's compound
        // SELECT limit while keeping database round trips bounded by page count.
        if targets.len() > 200 {
            let mut result = Vec::with_capacity(targets.len());
            for batch in targets.chunks(200) {
                result.extend(self.ignored_in_targets(batch)?);
            }
            return Ok(result);
        }
        let mut ids = Vec::new();
        let mut parts = Vec::new();
        for (i, target) in targets.iter().enumerate() {
            let (selection, id) = tracks(target);
            ids.push(id);
            let selection = selection
                .replace("?1", &format!("?{}", i + 1))
                .replace(" UNION ", " UNION ALL ");
            parts.push(format!("SELECT {i} ordinal, EXISTS(SELECT 1 FROM ({selection})) AND NOT EXISTS(SELECT 1 FROM ({selection}) t WHERE NOT EXISTS(SELECT 1 FROM ignored_track_preference p WHERE p.profile_id='local' AND p.track_id=t.id)) ignored"));
        }
        let mut statement = self.store.connection.prepare(&parts.join(" UNION ALL "))?;
        Ok(statement
            .query_map(rusqlite::params_from_iter(ids), |r| r.get(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
}
fn eligible(c: &rusqlite::Connection, track: &TrackId) -> Result<bool> {
    Ok(c.query_row("SELECT NOT EXISTS(SELECT 1 FROM ignored_track_preference WHERE profile_id='local' AND track_id=?1)",params![track.as_ref()],|r|r.get(0))?)
}
impl QueueReader {
    pub fn filter_generated_tracks(
        &self,
        rows: Vec<crate::domain::TrackSearchResult>,
        explicit: &[String],
    ) -> Result<Vec<crate::domain::TrackSearchResult>> {
        Ok(self.prepare_playback_program(rows, 0, explicit, false)?.0)
    }
    pub fn prepare_playback_program(
        &self,
        rows: Vec<crate::domain::TrackSearchResult>,
        position: usize,
        explicit: &[String],
        explicit_start_only: bool,
    ) -> Result<(Vec<crate::domain::TrackSearchResult>, usize)> {
        let ids =
            serde_json::to_string(&rows.iter().map(|r| r.track_id.as_ref()).collect::<Vec<_>>())
                .expect("serialize Track IDs");
        let mut statement=self.0.prepare("SELECT track_id FROM ignored_track_preference WHERE profile_id='local' AND track_id IN (SELECT value FROM json_each(?1))")?;
        let ignored = statement
            .query_map([ids], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        let explicit = explicit.iter().collect::<std::collections::HashSet<_>>();
        let mut selected = 0;
        let mut result = Vec::with_capacity(rows.len());
        for (i, row) in rows.into_iter().enumerate() {
            let allowed = if explicit_start_only {
                i == position
            } else {
                explicit.contains(&row.track_id.0)
            };
            if allowed || !ignored.contains(&row.track_id.0) {
                if i == position {
                    selected = result.len();
                }
                result.push(row);
            }
        }
        Ok((result, selected))
    }
}
