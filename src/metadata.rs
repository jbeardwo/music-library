//! Offline metadata inspection, sparse user overrides, and explicit persisted candidate connection.
use crate::{
    Library,
    domain::{SourceId, TrackId},
    storage::{Error, Result},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Track(String),
    Album(String),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub field: String,
    /// None clears the override; Some("") explicitly blanks an optional field.
    pub value: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub value: String,
    pub overridden: bool,
    pub effective_source: String,
    pub editable: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct Evidence {
    pub candidate_id: Option<String>,
    pub track_id: Option<String>,
    pub source: String,
    pub label: String,
    pub values: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct TrackEvidence {
    pub track_id: String,
    pub title: String,
    pub disc_number: Option<i64>,
    pub track_number: Option<i64>,
    pub evidence: Vec<Evidence>,
}
#[derive(Clone, Debug, Serialize)]
pub struct LocalFile {
    pub source_id: String,
    pub track_id: String,
    pub path: String,
    pub available: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct ArtistChoice {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Inspection {
    pub assigned_artist: Option<ArtistChoice>,
    pub target: Target,
    pub album_id: String,
    pub fields: Vec<Field>,
    pub evidence: Vec<Evidence>,
    pub track_evidence: Vec<TrackEvidence>,
    pub files: Vec<LocalFile>,
    pub track_count: usize,
    pub identities: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct FileOutcome {
    pub source_id: String,
    pub path: String,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct SaveOutcome {
    pub tracks: usize,
    pub track_ids: Vec<String>,
    pub files: Vec<FileOutcome>,
    pub destination_album: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RenameMatch {
    pub album_id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<i64>,
    pub track_count: usize,
    pub release_count: usize,
    pub blocked_reason: String,
}

const TRACK_FIELDS: &[(&str, &str)] = &[
    ("title", "Track title"),
    ("artist_credit", "Displayed Artist credit"),
    ("genre", "Genre"),
    ("year", "Year"),
    ("disc_number", "Disc number"),
    ("track_number", "Track number"),
];
const ALBUM_FIELDS: &[(&str, &str)] = &[
    ("title", "Album title"),
    ("artist_credit", "Displayed Album artist credit"),
    ("genre", "Genre"),
    ("year", "Year"),
    ("release_type", "Release type"),
];
fn specs(target: &Target) -> &'static [(&'static str, &'static str)] {
    match target {
        Target::Track(_) => TRACK_FIELDS,
        Target::Album(_) => ALBUM_FIELDS,
    }
}
fn id(target: &Target) -> &str {
    match target {
        Target::Track(id) | Target::Album(id) => id,
    }
}
fn invalid(s: impl Into<String>) -> Error {
    Error::Invalid(s.into())
}
fn list(db: &Connection, target: &Target) -> Result<Vec<TrackId>> {
    let sql = match target {
        Target::Track(_) => "SELECT id FROM track WHERE id=?1",
        Target::Album(_) => {
            "SELECT t.id FROM release r JOIN track t ON t.release_id=r.id WHERE r.album_id=?1 ORDER BY t.id"
        }
    };
    Ok(db
        .prepare(sql)?
        .query_map([id(target)], |r| Ok(TrackId(r.get(0)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}
pub(crate) fn override_value(
    db: &Connection,
    target: &Target,
    key: &str,
) -> Result<Option<String>> {
    let (table, column, pk) = match target {
        Target::Track(_) if key == "title" => ("track_title_override", "value", "track_id"),
        Target::Track(_) => ("track_metadata_override", key, "track_id"),
        Target::Album(_) => ("album_metadata_override", key, "album_id"),
    };
    let expression = if ["year", "disc_number", "track_number"].contains(&key) {
        format!("CASE WHEN {column}_set THEN COALESCE(CAST({column} AS TEXT),'') ELSE NULL END")
    } else {
        column.into()
    };
    Ok(db
        .query_row(
            &format!("SELECT {expression} FROM {table} WHERE {pk}=?1"),
            [id(target)],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}
pub(crate) fn credit(db: &Connection, scope: &str, id: &str) -> Result<String> {
    let credits=db.prepare(&format!("SELECT COALESCE(c.credited_name,a.name),c.join_phrase FROM {scope}_artist_credit c JOIN artist a ON a.id=c.artist_id WHERE c.{scope}_id=?1 ORDER BY c.position"))?.query_map([id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let count = credits.len();
    Ok(credits
        .iter()
        .enumerate()
        .map(|(i, (name, phrase))| {
            format!(
                "{}{}",
                name,
                phrase.as_deref().unwrap_or(if i + 1 < count {
                    if scope == "album" { " / " } else { ", " }
                } else {
                    ""
                })
            )
        })
        .collect())
}
fn add(values: &mut BTreeMap<String, String>, key: &str, value: Option<String>) {
    if let Some(v) = value.filter(|v| !v.is_empty()) {
        values.insert(key.into(), v);
    }
}
fn number(value: Option<i64>) -> Option<String> {
    value.map(|v| v.to_string())
}

impl Library {
    /// Indexed, bounded canonical name lookup; callers select IDs when names are ambiguous.
    pub fn metadata_artist_choices(&self, name: &str) -> Result<Vec<ArtistChoice>> {
        let name = name.trim();
        Ok(self.store.connection.prepare("SELECT id,name FROM artist WHERE lower(name)>=lower(?1) AND lower(name)<lower(?1)||char(1114111) ORDER BY lower(name),id LIMIT 20")?.query_map([name], |r| Ok(ArtistChoice { id:r.get(0)?, name:r.get(1)? }))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Suggestions only: title equality never moves anything without an explicit choice.
    pub fn metadata_album_rename_matches(
        &self,
        target: &Target,
        changes: &[Change],
    ) -> Result<Vec<RenameMatch>> {
        let changes = validate(target, changes)?;
        let Target::Album(source) = target else {
            return Ok(Vec::new());
        };
        let Some(title) = rename_title(&self.store.connection, source, &changes)? else {
            return Ok(Vec::new());
        };
        let sql = format!(
            "SELECT a.album_id,a.title,{},a.year,(SELECT count(*) FROM release r JOIN track t ON t.release_id=r.id WHERE r.album_id=a.album_id),(SELECT count(*) FROM release r WHERE r.album_id=a.album_id),EXISTS(SELECT 1 FROM album_external_identity s JOIN album_external_identity d ON d.album_id=a.album_id AND d.provider=s.provider AND d.kind=s.kind WHERE s.album_id=?2 AND s.external_id<>d.external_id) FROM album_browse_order b INDEXED BY album_order_title JOIN effective_album_metadata a ON a.album_id=b.album_id WHERE b.title_key=lower(?1) AND a.album_id<>?2 AND EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id WHERE r.album_id=a.album_id) ORDER BY b.album_id LIMIT 20",
            crate::browse::album_artist_credit_sql()
        );
        Ok(self.store.connection.prepare(&sql)?.query_map(params![title,source],|r| Ok(RenameMatch {
            album_id:r.get(0)?,title:r.get(1)?,artist:r.get(2)?,year:r.get(3)?,track_count:r.get::<_,i64>(4)? as usize,release_count:r.get::<_,i64>(5)? as usize,
            blocked_reason:if r.get::<_,bool>(6)? { "These Albums have conflicting trusted provider identities. Keep them separate or resolve the identity conflict first.".into() } else { String::new() },
        }))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Indexed SQLite reads only. All attached local observations, including missing files.
    pub fn inspect_metadata(&self, target: &Target) -> Result<Inspection> {
        inspect(&self.store.connection, target)
    }
    /// Bounded presentation refresh, including canonical queued Tracks outside membership.
    pub fn metadata_track_labels(
        &self,
        tracks: &[String],
    ) -> Result<Vec<crate::domain::TrackSearchResult>> {
        let ids = serde_json::to_string(tracks).map_err(|e| invalid(e.to_string()))?;
        Ok(self.store.connection.prepare("SELECT e.track_id,t.release_id,e.title,e.release_title,e.artist_names,e.year,EXISTS(SELECT 1 FROM track_source s JOIN local_file_observation l ON l.source_id=s.source_id WHERE s.track_id=t.id AND l.available=1) FROM json_each(?1) ids CROSS JOIN track t ON t.id=ids.value JOIN effective_track_metadata e ON e.track_id=t.id JOIN release r ON r.id=t.release_id JOIN effective_album_metadata a ON a.album_id=r.album_id".replace("e.artist_names", &crate::browse::song_artist_credit_sql()).as_str())?.query_map([ids],|r|Ok(crate::domain::TrackSearchResult{track_id:TrackId(r.get(0)?),release_id:crate::domain::ReleaseId(r.get(1)?),title:r.get(2)?,release_title:r.get(3)?,artist_names:r.get(4)?,year:r.get(5)?,available:r.get(6)?}))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Explicitly connect one persisted Spotify candidate through the existing identity workflow.
    pub fn connect_metadata_spotify_candidate(
        &mut self,
        track: &TrackId,
        external_id: &str,
    ) -> Result<crate::domain::ExternalIdentity> {
        let page: String = self
            .store
            .connection
            .query_row(
                "SELECT page_json FROM spotify_reconciliation_cache WHERE track_id=?1",
                [track.as_ref()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| invalid("Candidate evidence is no longer available"))?;
        let page: crate::catalog::Page<crate::song_resolution::Candidate> =
            serde_json::from_str(&page).map_err(|e| invalid(e.to_string()))?;
        let candidate = page
            .items
            .into_iter()
            .find(|c| {
                c.identity.provider == "spotify"
                    && c.identity.kind == "track"
                    && c.identity.external_id == external_id
            })
            .ok_or_else(|| invalid("Select a currently persisted Spotify Track candidate"))?;
        let input = self.song_resolution_input(track)?;
        let selection = crate::song_resolution::Selection::new(input, vec![candidate]);
        self.confirm_song_resolution(&selection, 0)
    }
    /// Selected source IDs are explicit authorization for each file; empty means DB only.
    /// Validate the entire request before committing, then perform independent file writes.
    pub fn save_metadata(
        &mut self,
        target: &Target,
        changes: &[Change],
        selected_files: &[String],
    ) -> Result<SaveOutcome> {
        self.save_metadata_with_album_move(target, changes, selected_files, None)
    }
    /// A chosen destination explicitly authorizes regrouping whole Releases, never merging Tracks.
    pub fn save_metadata_with_album_move(
        &mut self,
        target: &Target,
        changes: &[Change],
        selected_files: &[String],
        destination_album: Option<&str>,
    ) -> Result<SaveOutcome> {
        let changes = validate(target, changes)?;
        if changes.is_empty() {
            return Err(invalid("No metadata changes requested"));
        }
        let snapshot = self.inspect_metadata(target)?;
        let mut selected = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for source in selected_files {
            if !seen.insert(source) {
                continue;
            }
            selected.push(
                snapshot
                    .files
                    .iter()
                    .find(|f| &f.source_id == source)
                    .cloned()
                    .ok_or_else(|| {
                        invalid("Selected file is not attached to this metadata target")
                    })?,
            );
        }
        let tx = self.store.connection.transaction()?;
        let tracks = list(&tx, target)?;
        let matching = changes.iter().any(|c| c.field != "genre");
        for c in &changes {
            if c.field == "artist_assignment" || c.field == "artist_assignment_id" {
                continue;
            }
            if matches!(target, Target::Track(_)) && c.field == "title" {
                if let Some(value) = &c.value {
                    tx.execute("INSERT INTO track_title_override(track_id,value) VALUES(?1,?2) ON CONFLICT(track_id) DO UPDATE SET value=excluded.value,updated_at=unixepoch()",params![id(target),value])?;
                } else {
                    tx.execute(
                        "DELETE FROM track_title_override WHERE track_id=?1",
                        [id(target)],
                    )?;
                }
            } else {
                let (table, pk) = match target {
                    Target::Track(_) => ("track_metadata_override", "track_id"),
                    Target::Album(_) => ("album_metadata_override", "album_id"),
                };
                if ["year", "disc_number", "track_number"].contains(&c.field.as_str()) {
                    let numeric = c
                        .value
                        .as_ref()
                        .filter(|v| !v.is_empty())
                        .map(|v| v.parse::<i64>().expect("validated number"));
                    tx.execute(&format!("INSERT INTO {table}({pk},{0},{0}_set) VALUES(?1,?2,?3) ON CONFLICT({pk}) DO UPDATE SET {0}=excluded.{0},{0}_set=excluded.{0}_set,updated_at=unixepoch()",c.field),params![id(target),numeric,c.value.is_some()])?;
                } else {
                    tx.execute(&format!("INSERT INTO {table}({pk},{}) VALUES(?1,?2) ON CONFLICT({pk}) DO UPDATE SET {}=excluded.{},updated_at=unixepoch()",c.field,c.field,c.field),params![id(target),c.value])?;
                }
            }
        }
        reassign_artist(&tx, target, &changes)?;
        if let Some(destination) = destination_album {
            let Target::Album(source) = target else {
                return Err(invalid("Only an Album rename can move its Tracks"));
            };
            let title = rename_title(&tx, source, &changes)?
                .ok_or_else(|| invalid("Change the Album title before choosing a destination"))?;
            move_album_releases(&tx, source, destination, &title)?;
        }
        for t in &tracks {
            crate::storage::refresh_effective_track_tx(&tx, t)?;
            if matching {
                tx.execute("UPDATE spotify_connection_review SET stale=1 WHERE track_id=?1 AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=spotify_connection_review.track_id) AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=spotify_connection_review.track_id AND x.provider='spotify')",[t.as_ref()])?;
            }
        }
        if let Target::Album(album) = target {
            // Refresh matching keys after either credit or relationship edits.
            let title: String = tx.query_row(
                "SELECT title FROM effective_album_metadata WHERE album_id=?1",
                [album],
                |r| r.get(0),
            )?;
            let artist = override_value(&tx, target, "artist_credit")?
                .unwrap_or(credit(&tx, "album", album)?);
            tx.execute("UPDATE album_application_metadata SET match_title=?2,match_artist_credit=?3 WHERE album_id=?1",params![album,crate::matching::normalize(&title),crate::matching::normalize(&artist)])?;
        }
        if let Some(destination) = destination_album {
            for album in [id(target), destination] {
                tx.execute(
                    "UPDATE album_application_metadata SET year=year WHERE album_id=?1",
                    [album],
                )?;
            }
        }
        tx.commit()?;
        let mut outcomes = Vec::new();
        let display_target = destination_album
            .map(|a| Target::Album(a.into()))
            .unwrap_or_else(|| target.clone());
        let current = self.inspect_metadata(&display_target)?;
        let mut write_values = changes
            .iter()
            .map(|c| {
                (
                    c.field.clone(),
                    current
                        .fields
                        .iter()
                        .find(|f| f.key == c.field)
                        .map(|f| f.value.clone())
                        .unwrap_or_default(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        write_values.remove("artist_assignment_id");
        write_values.remove("artist_assignment");
        if changes
            .iter()
            .any(|c| c.field == "artist_assignment" || c.field == "artist_assignment_id")
            && let Some(artist) = &current.assigned_artist
        {
            write_values.insert("artist_assignment".into(), artist.name.clone());
        }
        for file in selected {
            let result = (|| -> std::result::Result<(), String> {
                if !file.available {
                    return Err("Source is unavailable".into());
                }
                if matches!(target, Target::Album(_))
                    && changes
                        .iter()
                        .any(|c| c.field == "genre" && c.value.is_none())
                    && write_values.get("genre").is_some_and(|v| v.is_empty())
                {
                    return Err("Automatic Album Genre is mixed or missing; set an explicit shared Genre before bulk write-back".into());
                }
                // Write only the requested fields, using effective values after clearing overrides.
                let bytes = self
                    .store
                    .connection
                    .query_row(
                        "SELECT path FROM local_file_observation WHERE source_id=?1",
                        [&file.source_id],
                        |r| r.get::<_, Vec<u8>>(0),
                    )
                    .map_err(|e| e.to_string())?;
                let path = crate::storage::bytes_to_path(bytes);
                let mut file_values = write_values.clone();
                if matches!(target, Target::Album(_))
                    && file_values.contains_key("artist_assignment")
                {
                    let track_artist = assigned_artist(
                        &self.store.connection,
                        &Target::Track(file.track_id.clone()),
                    )
                    .map_err(|e| e.to_string())?;
                    if track_artist.as_ref().map(|a| &a.id)
                        != current.assigned_artist.as_ref().map(|a| &a.id)
                    {
                        let value = file_values.remove("artist_assignment").unwrap();
                        file_values.insert("artist_assignment_album_only".into(), value);
                    }
                }
                write_tags(&path, target, &file_values)?;
                // Re-read with the normal parser and persist evidence against the SAME source ID.
                use crate::filesystem::MetadataExtractor;
                let observation = crate::filesystem::LoftyMetadataExtractor
                    .read(&path)
                    .map_err(|e| e.to_string())?;
                let tx = self
                    .store
                    .connection
                    .transaction()
                    .map_err(|e| e.to_string())?;
                crate::storage::write_file_metadata(
                    &tx,
                    &SourceId(file.source_id.clone()),
                    &observation,
                    Some(
                        &serde_json::to_string(&observation.provenance)
                            .map_err(|e| e.to_string())?,
                    ),
                )
                .map_err(|e| e.to_string())?;
                crate::storage::refresh_effective_track_tx(&tx, &TrackId(file.track_id.clone()))
                    .map_err(|e| e.to_string())?;
                tx.commit().map_err(|e| e.to_string())?;
                Ok(())
            })();
            outcomes.push(FileOutcome {
                source_id: file.source_id,
                path: file.path,
                error: result.err(),
            });
        }
        Ok(SaveOutcome {
            tracks: tracks.len(),
            track_ids: tracks.iter().map(|t| t.0.clone()).collect(),
            files: outcomes,
            destination_album: destination_album.map(str::to_owned),
        })
    }
}
fn assigned_artist(db: &Connection, target: &Target) -> Result<Option<ArtistChoice>> {
    let scope = if matches!(target, Target::Track(_)) {
        "track"
    } else {
        "album"
    };
    let direct = db.query_row(&format!("SELECT a.id,a.name FROM {scope}_artist_credit c JOIN artist a ON a.id=c.artist_id WHERE c.{scope}_id=?1 ORDER BY position LIMIT 1"), [id(target)], |r| Ok(ArtistChoice { id:r.get(0)?, name:r.get(1)? })).optional()?;
    if direct.is_some() || matches!(target, Target::Album(_)) {
        return Ok(direct);
    }
    let release = db.query_row("SELECT a.id,a.name FROM track t JOIN release_artist_credit c ON c.release_id=t.release_id JOIN artist a ON a.id=c.artist_id WHERE t.id=?1 ORDER BY c.position LIMIT 1",[id(target)],|r|Ok(ArtistChoice{id:r.get(0)?,name:r.get(1)?})).optional()?;
    if release.is_some() {
        return Ok(release);
    }
    let album: String = db.query_row(
        "SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id=?1",
        [id(target)],
        |r| r.get(0),
    )?;
    assigned_artist(db, &Target::Album(album))
}
fn reassign_artist(db: &Connection, target: &Target, changes: &[Change]) -> Result<()> {
    let name = changes
        .iter()
        .find(|c| c.field == "artist_assignment")
        .and_then(|c| c.value.as_deref());
    let selected = changes
        .iter()
        .find(|c| c.field == "artist_assignment_id")
        .and_then(|c| c.value.as_deref());
    if name.is_none() && selected.is_none() {
        return Ok(());
    }
    let artist = if let Some(selected) = selected {
        db.query_row("SELECT id,name FROM artist WHERE id=?1", [selected], |r| {
            Ok(ArtistChoice {
                id: r.get(0)?,
                name: r.get(1)?,
            })
        })
        .optional()?
        .ok_or_else(|| invalid("Selected Artist no longer exists"))?
    } else {
        let name = name.unwrap();
        let matches = db
            .prepare(
                "SELECT id,name FROM artist WHERE lower(name)=lower(?1) AND name=?1 ORDER BY id LIMIT 2",
            )?
            .query_map([name], |r| {
                Ok(ArtistChoice {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        match matches.len() {
            0 => {
                let artist = ArtistChoice {
                    id: crate::domain::ArtistId::new().0,
                    name: name.into(),
                };
                db.execute(
                    "INSERT INTO artist(id,name) VALUES(?1,?2)",
                    params![artist.id, artist.name],
                )?;
                artist
            }
            1 => matches.into_iter().next().unwrap(),
            _ => {
                return Err(invalid(
                    "Multiple Artists have this exact name; select the intended Artist ID",
                ));
            }
        }
    };
    let old = assigned_artist(db, target)?;
    if old.as_ref().is_some_and(|a| a.id == artist.id) {
        return Ok(());
    }
    let scope = if matches!(target, Target::Track(_)) {
        "track"
    } else {
        "album"
    };
    if let Some(old) = old {
        let updated = db.execute(&format!("UPDATE {scope}_artist_credit SET artist_id=?2,credited_name=CASE WHEN credited_name IS NULL OR credited_name=?3 THEN ?4 ELSE credited_name END WHERE {scope}_id=?1 AND position=(SELECT min(position) FROM {scope}_artist_credit WHERE {scope}_id=?1)"), params![id(target),artist.id,old.name,artist.name])?;
        if updated == 0 {
            db.execute(&format!("INSERT INTO {scope}_artist_credit({scope}_id,position,artist_id,credited_name) VALUES(?1,0,?2,?3)"),params![id(target),artist.id,artist.name])?;
        }
        if matches!(target, Target::Album(_)) {
            for (scope, filter) in [
                (
                    "track",
                    "track_id IN(SELECT t.id FROM release r JOIN track t ON t.release_id=r.id WHERE r.album_id=?1)",
                ),
                (
                    "release",
                    "release_id IN(SELECT id FROM release WHERE album_id=?1)",
                ),
            ] {
                db.execute(&format!("UPDATE {scope}_artist_credit SET artist_id=?2,credited_name=CASE WHEN credited_name IS NULL OR credited_name=?4 THEN ?5 ELSE credited_name END WHERE {filter} AND (artist_id=?3 OR (position=(SELECT min(other.position) FROM {scope}_artist_credit other WHERE other.{scope}_id={scope}_artist_credit.{scope}_id) AND artist_id IN(SELECT id FROM artist WHERE lower(name)=lower(?4) AND name=?4)))"),params![id(target),artist.id,old.id,old.name,artist.name])?;
            }
        }
    } else {
        db.execute(&format!("INSERT INTO {scope}_artist_credit({scope}_id,position,artist_id,credited_name) VALUES(?1,0,?2,?3)"),params![id(target),artist.id,artist.name])?;
    }
    if matches!(target, Target::Album(_)) {
        // Missing credits inherit the Album/Release. Materialize only that primary assignment.
        db.execute("INSERT INTO release_artist_credit(release_id,position,artist_id,credited_name) SELECT id,0,?2,?3 FROM release r WHERE album_id=?1 AND NOT EXISTS(SELECT 1 FROM release_artist_credit c WHERE c.release_id=r.id)",params![id(target),artist.id,artist.name])?;
        db.execute("INSERT INTO track_artist_credit(track_id,position,artist_id,credited_name) SELECT t.id,0,?2,?3 FROM release r JOIN track t ON t.release_id=r.id WHERE r.album_id=?1 AND NOT EXISTS(SELECT 1 FROM track_artist_credit c WHERE c.track_id=t.id) AND EXISTS(SELECT 1 FROM release_artist_credit c WHERE c.release_id=r.id AND c.artist_id=?2 AND c.position=(SELECT min(position) FROM release_artist_credit WHERE release_id=r.id))",params![id(target),artist.id,artist.name])?;
    }
    Ok(())
}
fn rename_title(db: &Connection, source: &str, changes: &[Change]) -> Result<Option<String>> {
    let Some(change) = changes.iter().find(|c| c.field == "title") else {
        return Ok(None);
    };
    Ok(Some(match &change.value {
        Some(title) => title.clone(),
        None => db.query_row(
            "SELECT title FROM album_application_metadata WHERE album_id=?1",
            [source],
            |r| r.get(0),
        )?,
    }))
}
fn move_album_releases(
    tx: &rusqlite::Transaction<'_>,
    source: &str,
    destination: &str,
    title: &str,
) -> Result<()> {
    if source == destination {
        return Err(invalid("Choose another existing Album"));
    }
    let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM album_browse_order b WHERE b.album_id=?1 AND b.title_key=lower(?2) AND EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id WHERE r.album_id=b.album_id))",params![destination,title],|r|r.get(0))?;
    if !valid {
        return Err(invalid(
            "The existing Album changed or is no longer available; choose again",
        ));
    }
    let conflict: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM album_external_identity s JOIN album_external_identity d ON d.album_id=?2 AND d.provider=s.provider AND d.kind=s.kind WHERE s.album_id=?1 AND s.external_id<>d.external_id)",params![source,destination],|r|r.get(0))?;
    if conflict {
        return Err(invalid(
            "Conflicting trusted Album identities; the Albums were kept separate",
        ));
    }
    // Transfer identity ownership with its provenance markers before reparenting.
    // Track/Release identities and their association records never change.
    let identities = tx.prepare("SELECT i.provider,i.kind,i.external_id,EXISTS(SELECT 1 FROM album_provenance_identity p WHERE p.album_id=i.album_id AND p.provider=i.provider AND p.kind=i.kind AND p.external_id=i.external_id),EXISTS(SELECT 1 FROM album_external_identity d WHERE d.album_id=?2 AND d.provider=i.provider AND d.kind=i.kind AND d.external_id=i.external_id) FROM album_external_identity i WHERE i.album_id=?1")?.query_map(params![source,destination],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,bool>(3)?,r.get::<_,bool>(4)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (provider, kind, external, provenance, existing) in identities {
        tx.execute("INSERT OR IGNORE INTO album_external_identity(album_id,provider,kind,external_id) VALUES(?1,?2,?3,?4)",params![destination,provider,kind,external])?;
        if provenance && !existing {
            tx.execute("INSERT OR IGNORE INTO album_provenance_identity(album_id,provider,kind,external_id) VALUES(?1,?2,?3,?4)",params![destination,provider,kind,external])?;
        } else if !provenance {
            tx.execute("DELETE FROM album_provenance_identity WHERE album_id=?1 AND provider=?2 AND kind=?3 AND external_id=?4",params![destination,provider,kind,external])?;
        }
    }
    tx.execute(
        "DELETE FROM album_external_identity WHERE album_id=?1",
        [source],
    )?;
    // Keep original evidence/overrides on the now-empty source entity as well.
    tx.execute("INSERT OR IGNORE INTO album_provider_evidence SELECT ?2,provider,kind,external_id,title,release_date,release_type FROM album_provider_evidence WHERE album_id=?1",params![source,destination])?;
    tx.execute("INSERT OR IGNORE INTO album_artwork SELECT ?2,origin,locator,cache_name,checked_at FROM album_artwork WHERE album_id=?1",params![source,destination])?;
    tx.execute(
        "UPDATE release SET album_id=?2 WHERE album_id=?1",
        params![source, destination],
    )?;
    tx.execute(
        "DELETE FROM spotify_program_cache WHERE album_id IN(?1,?2)",
        params![source, destination],
    )?;
    for album in [source, destination] {
        crate::storage::refresh_album_match_key(tx, album)?;
        tx.execute(
            "UPDATE album_application_metadata SET year=year WHERE album_id=?1",
            [album],
        )?;
    }
    Ok(())
}
fn validate(target: &Target, changes: &[Change]) -> Result<Vec<Change>> {
    if changes.iter().any(|c| c.field == "artist_assignment")
        && changes.iter().any(|c| c.field == "artist_assignment_id")
    {
        return Err(invalid(
            "Choose an Artist ID or enter an exact name, not both",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    changes.iter().map(|c| {
        if !specs(target).iter().any(|(k,_)|*k==c.field) && !["artist_assignment","artist_assignment_id"].contains(&c.field.as_str()) || !seen.insert(&c.field) {return Err(invalid("Unknown or repeated metadata field"));}
        let value=c.value.as_ref().map(|v|v.trim().to_owned());
        if ["artist_assignment","artist_assignment_id"].contains(&c.field.as_str()) && value.as_ref().is_none_or(|v| v.is_empty()) { return Err(invalid("Choose an Artist or enter its exact name")); }
        if let Some(v)=&value {
            if v.len()>4096 || v.contains('\0') {return Err(invalid("Metadata value is too long or contains NUL"));}
            if ["title","artist_credit"].contains(&c.field.as_str()) && v.is_empty() {return Err(invalid("Title and artist-credit overrides cannot be blank; use automatic value to clear an override"));}
            if ["year","disc_number","track_number"].contains(&c.field.as_str()) && !v.is_empty() {
                let n=v.parse::<u32>().map_err(|_|invalid("Expected a positive whole number"))?;
                if n==0 || n>if c.field=="year" {9999}else{99999} {return Err(invalid("Number outside supported range"));}
            }
        }
        Ok(Change{field:c.field.clone(),value})
    }).collect()
}

pub(crate) fn refresh_track_fields(db: &Connection, track: &TrackId) -> Result<()> {
    db.execute("UPDATE effective_track_metadata SET disc_number=(SELECT CASE WHEN o.disc_number_set THEN o.disc_number ELSE t.disc_number END FROM track t LEFT JOIN track_metadata_override o ON o.track_id=t.id WHERE t.id=?1),track_number=(SELECT CASE WHEN o.track_number_set THEN o.track_number ELSE t.track_number END FROM track t LEFT JOIN track_metadata_override o ON o.track_id=t.id WHERE t.id=?1) WHERE track_id=?1",[track.as_ref()])?;
    // Shared Album correction replaces only equal/absent automatic Track credits.
    let album: String = db.query_row(
        "SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id=?1",
        [track.as_ref()],
        |r| r.get(0),
    )?;
    if override_value(db, &Target::Track(track.0.clone()), "artist_credit")?.is_none()
        && let Some(value) = override_value(db, &Target::Album(album.clone()), "artist_credit")?
    {
        let track_credit = credit(db, "track", track.as_ref())?;
        if track_credit.is_empty() || track_credit == credit(db, "album", &album)? {
            db.execute(
                "UPDATE effective_track_metadata SET artist_names=?2 WHERE track_id=?1",
                params![track.as_ref(), value],
            )?;
        }
    }
    db.execute(
        "DELETE FROM effective_track_genre WHERE track_id=?1",
        [track.as_ref()],
    )?;
    let genres = override_value(db, &Target::Track(track.0.clone()), "genre")?.or(override_value(
        db,
        &Target::Album(album),
        "genre",
    )?);
    if let Some(genres) = genres {
        for genre in genres.split('·').map(str::trim).filter(|v| !v.is_empty()) {
            db.execute(
                "INSERT OR IGNORE INTO effective_track_genre VALUES(?1,?2)",
                params![track.as_ref(), genre],
            )?;
        }
    } else {
        db.execute("INSERT OR IGNORE INTO effective_track_genre SELECT ts.track_id,g.genre FROM track_source ts JOIN file_genre_observation g ON g.source_id=ts.source_id WHERE ts.track_id=?1",[track.as_ref()])?;
    }
    Ok(())
}

fn inspect(db: &Connection, target: &Target) -> Result<Inspection> {
    let tracks = list(db, target)?;
    let album = match target {
        Target::Album(a) => {
            if !db.query_row("SELECT EXISTS(SELECT 1 FROM album WHERE id=?1)", [a], |r| {
                r.get::<_, bool>(0)
            })? {
                return Err(invalid("Unknown Album"));
            }
            a.clone()
        }
        Target::Track(t) => db
            .query_row(
                "SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id=?1",
                [t],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| invalid("Unknown Track"))?,
    };
    let mut effective = BTreeMap::new();
    if let Target::Track(t) = target {
        let row=db.query_row("SELECT e.title,e.artist_names,e.genre_names,e.year,e.disc_number,e.track_number,e.duration_ms,e.release_title FROM effective_track_metadata e JOIN track t ON t.id=e.track_id JOIN release r ON r.id=t.release_id JOIN effective_album_metadata a ON a.album_id=r.album_id WHERE track_id=?1".replace("e.artist_names", &crate::browse::song_artist_credit_sql()).as_str(),[t],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<i64>>(3)?,r.get::<_,Option<i64>>(4)?,r.get::<_,Option<i64>>(5)?,r.get::<_,Option<i64>>(6)?,r.get::<_,String>(7)?)))?;
        effective.insert("title".into(), row.0);
        effective.insert("artist_credit".into(), row.1);
        effective.insert("genre".into(), row.2);
        add(&mut effective, "year", number(row.3));
        add(&mut effective, "disc_number", number(row.4));
        add(&mut effective, "track_number", number(row.5));
        add(
            &mut effective,
            "duration",
            row.6
                .map(|v| format!("{}:{:02}.{:03}", v / 60000, v / 1000 % 60, v % 1000)),
        );
        effective.insert("album".into(), row.7);
    } else {
        let (title,year,artist,genre,kind)=db.query_row("SELECT title,year,artist_credit,genre,release_type FROM effective_album_metadata WHERE album_id=?1",[&album],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<i64>>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,Option<String>>(4)?)))?;
        effective.insert("title".into(), title);
        add(&mut effective, "year", number(year));
        effective.insert(
            "artist_credit".into(),
            artist.unwrap_or(credit(db, "album", &album)?),
        );
        add(&mut effective, "genre", genre);
        add(
            &mut effective,
            "release_type",
            kind.or(automatic_release_type(db, &album)?),
        );
        // With no shared override, represent differing Track genres rather than selecting a file.
        if !effective.contains_key("genre") {
            let values=db.prepare("SELECT DISTINCT e.genre_names FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=?1 AND e.genre_names<>'' ORDER BY e.genre_names")?.query_map([&album],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            effective.insert(
                "genre".into(),
                if values.len() == 1 {
                    values[0].clone()
                } else {
                    String::new()
                },
            );
        }
    }
    let filter = match target {
        Target::Track(_) => "ts.track_id=?1",
        Target::Album(_) => "r.album_id=?1",
    };
    let mut local_credits = BTreeMap::<(String, String), Vec<String>>::new();
    let mut query=db.prepare(&format!("SELECT ts.source_id,f.scope,f.name FROM track_source ts JOIN track t ON t.id=ts.track_id JOIN release r ON r.id=t.release_id JOIN file_artist_observation f ON f.source_id=ts.source_id WHERE {filter} ORDER BY ts.source_id,f.scope,f.position"))?;
    for row in query.query_map([id(target)], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (s, scope, name) = row?;
        local_credits.entry((s, scope)).or_default().push(name);
    }
    let mut local_genres = BTreeMap::<String, Vec<String>>::new();
    let mut query=db.prepare(&format!("SELECT ts.source_id,g.genre FROM track_source ts JOIN track t ON t.id=ts.track_id JOIN release r ON r.id=t.release_id JOIN file_genre_observation g ON g.source_id=ts.source_id WHERE {filter} ORDER BY ts.source_id,g.genre"))?;
    for row in query.query_map([id(target)], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (s, g) = row?;
        local_genres.entry(s).or_default().push(g);
    }
    let mut files = Vec::new();
    let mut evidence = Vec::new();
    let sql = format!(
        "SELECT ts.source_id,ts.track_id,l.path,l.available,f.track_title,f.release_title,f.year,f.disc_number,f.track_number,f.duration_ms FROM track_source ts JOIN track t ON t.id=ts.track_id JOIN release r ON r.id=t.release_id JOIN local_file_observation l ON l.source_id=ts.source_id LEFT JOIN file_metadata_observation f ON f.source_id=ts.source_id WHERE {filter} ORDER BY ts.track_id,ts.source_id"
    );
    let mut q = db.prepare(&sql)?;
    let rows = q.query_map([id(target)], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Vec<u8>>(2)?,
            r.get::<_, bool>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, Option<i64>>(7)?,
            r.get::<_, Option<i64>>(8)?,
            r.get::<_, Option<i64>>(9)?,
        ))
    })?;
    for row in rows {
        let (source, track, path, available, title, release, year, disc, position, duration) = row?;
        let path = crate::storage::bytes_to_path(path)
            .to_string_lossy()
            .into_owned();
        let mut values = BTreeMap::new();
        add(&mut values, "Track title", title);
        add(&mut values, "Album", release);
        add(&mut values, "Year", number(year));
        add(&mut values, "Disc number", number(disc));
        add(&mut values, "Track number", number(position));
        add(&mut values, "Duration (ms)", number(duration));
        for (scope, label) in [
            ("track", "Artist credit"),
            ("release", "Album artist credit"),
        ] {
            add(
                &mut values,
                label,
                local_credits
                    .get(&(source.clone(), scope.to_string()))
                    .map(|v| v.join(", ")),
            );
        }
        add(
            &mut values,
            "Genre",
            local_genres.get(&source).map(|v| v.join(" · ")),
        );
        evidence.push(Evidence {
            candidate_id: None,
            track_id: Some(track.clone()),
            source: "Local file".into(),
            label: format!("{}{}", path, if available { "" } else { " (unavailable)" }),
            values,
        });
        files.push(LocalFile {
            source_id: source,
            track_id: track,
            path,
            available,
        });
    }
    // Each exact provider object retains its own card and ID; no provider consensus.
    let mut q=db.prepare("SELECT provider,kind,external_id,title,release_date,release_type FROM album_provider_evidence WHERE album_id=?1 ORDER BY provider,kind,external_id")?;
    for row in q.query_map([&album], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
        ))
    })? {
        let (provider, kind, external, title, date, release_type) = row?;
        let mut values = BTreeMap::new();
        values.insert("Album".into(), title);
        add(&mut values, "Release date", date);
        add(&mut values, "Release type", release_type);
        evidence.push(Evidence {
            candidate_id: None,
            track_id: None,
            source: provider_name(&provider),
            label: format!("{kind}: {external}"),
            values,
        });
    }
    let track_filter = match target {
        Target::Track(_) => "e.track_id=?1",
        Target::Album(_) => "r.album_id=?1",
    };
    let mut q=db.prepare(&format!("SELECT e.provider,e.kind,e.external_id,e.title,e.disc,e.position,e.duration_ms,e.track_id FROM track_provider_evidence e JOIN track t ON t.id=e.track_id JOIN release r ON r.id=t.release_id WHERE {track_filter} ORDER BY e.provider,e.track_id,e.external_id"))?;
    for row in q.query_map([id(target)], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, Option<i64>>(5)?,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, String>(7)?,
        ))
    })? {
        let (provider, kind, external, title, disc, position, duration, track) = row?;
        let mut values = BTreeMap::new();
        values.insert("Track title".into(), title);
        add(&mut values, "Disc number", number(disc));
        add(&mut values, "Track number", number(position));
        add(&mut values, "Duration (ms)", number(duration));
        evidence.push(Evidence {
            candidate_id: None,
            track_id: Some(track),
            source: provider_name(&provider),
            label: format!("{kind}: {external}"),
            values,
        });
    }
    let mut query=db.prepare("SELECT r.id,m.title,m.year,COALESCE((SELECT group_concat(provider || ' / ' || kind || ': ' || external_id,char(10)) FROM release_external_identity i WHERE i.release_id=r.id),'') FROM release r JOIN release_application_metadata m ON m.release_id=r.id WHERE r.album_id=?1 ORDER BY r.id")?;
    for row in query.query_map([&album], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, String>(3)?,
        ))
    })? {
        let (release, title, year, identities) = row?;
        let mut values = BTreeMap::new();
        values.insert("Exact Release title".into(), title);
        add(&mut values, "Release year", number(year));
        add(&mut values, "Attached Release identities", Some(identities));
        evidence.push(Evidence{candidate_id:None,track_id:None,source:"Application".into(),label:format!("Exact Release snapshot · {release} (shared Album edits do not rewrite this evidence)"),values});
    }
    let association_filter = match target {
        Target::Track(_) => "m.track_id=?1",
        Target::Album(_) => "r.album_id=?1",
    };
    for (table, column) in [
        ("manual_track_association", "candidate_json"),
        ("provider_track_association", "match_json"),
    ] {
        let mut query=db.prepare(&format!("SELECT m.album_provider,m.album_kind,m.album_external_id,m.{column},m.track_id FROM {table} m JOIN track t ON t.id=m.track_id JOIN release r ON r.id=t.release_id WHERE {association_filter} ORDER BY m.album_provider,m.track_id"))?;
        for row in query.query_map([id(target)], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })? {
            let (provider, kind, external, json, track) = row?;
            let json: serde_json::Value =
                serde_json::from_str(&json).map_err(|e| invalid(e.to_string()))?;
            let item = json.get("evidence").unwrap_or(&json);
            let mut values = BTreeMap::new();
            add(
                &mut values,
                "Track title",
                item.get("title")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
            );
            for (key, label) in [
                ("disc", "Disc number"),
                ("number", "Track number"),
                ("duration_ms", "Duration (ms)"),
            ] {
                add(
                    &mut values,
                    label,
                    item.get(key)
                        .and_then(|v| v.as_u64())
                        .map(|v| v.to_string()),
                );
            }
            if let Some(a) = item.get("artists").and_then(|a| a.as_array()) {
                add(
                    &mut values,
                    "Artist credit",
                    Some(
                        a.iter()
                            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                );
            }
            for key in ["identities", "occurrences"] {
                if let Some(ids) = item.get(key).and_then(|v| v.as_array()) {
                    for identity in ids {
                        if let (Some(kind), Some(external)) = (
                            identity.get("kind").and_then(|v| v.as_str()),
                            identity.get("external_id").and_then(|v| v.as_str()),
                        ) {
                            values.insert(format!("ID ({kind})"), external.into());
                        }
                    }
                }
            }
            evidence.push(Evidence {
                candidate_id: None,
                track_id: Some(track),
                source: provider_name(&provider),
                label: format!("Persisted association · {kind}: {external}"),
                values,
            });
        }
    }
    // Last bounded Spotify search observations are useful offline, but are not trusted connections.
    let cache_filter = match target {
        Target::Track(_) => "c.track_id=?1",
        Target::Album(_) => "r.album_id=?1",
    };
    let mut query=db.prepare(&format!("SELECT c.track_id,c.page_json,EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=c.track_id) FROM spotify_reconciliation_cache c JOIN track t ON t.id=c.track_id JOIN release r ON r.id=t.release_id WHERE {cache_filter} ORDER BY c.track_id"))?;
    for row in query.query_map([id(target)], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, bool>(2)?,
        ))
    })? {
        let (track, json, connected) = row?;
        let page: crate::catalog::Page<crate::song_resolution::Candidate> =
            serde_json::from_str(&json).map_err(|e| invalid(e.to_string()))?;
        for candidate in page.items {
            let mut values = BTreeMap::new();
            values.insert("Track title".into(), candidate.title);
            values.insert("Artist credit".into(), candidate.artist);
            values.insert("Album".into(), candidate.album);
            add(&mut values, "Release date", Some(candidate.date));
            add(&mut values, "Release type", Some(candidate.album_type));
            evidence.push(Evidence {
                candidate_id: (!connected).then(|| candidate.identity.external_id.clone()),
                track_id: Some(track.clone()),
                source: "Spotify".into(),
                label: format!(
                    "Last persisted candidate (not a connection) · Track {track} · {}",
                    candidate.identity.external_id
                ),
                values,
            });
        }
    }
    // Keep the original application observations visible even while overridden.
    let mut values = BTreeMap::new();
    let (title, year): (String, Option<i64>) = db.query_row(
        "SELECT title,year FROM album_application_metadata WHERE album_id=?1",
        [&album],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    values.insert("Album".into(), title);
    add(&mut values, "Year", number(year));
    add(
        &mut values,
        "Album artist credit",
        Some(credit(db, "album", &album)?),
    );
    if let Target::Track(t) = target {
        let title: String = db.query_row(
            "SELECT title FROM track_application_metadata WHERE track_id=?1",
            [t],
            |r| r.get(0),
        )?;
        values.insert("Track title".into(), title);
        add(&mut values, "Artist credit", Some(credit(db, "track", t)?));
    }
    evidence.insert(
        0,
        Evidence {
            candidate_id: None,
            track_id: None,
            source: "Application".into(),
            label: "Automatic application metadata (underlying values)".into(),
            values,
        },
    );
    let mut identities = Vec::new();
    for (sql, key) in [
        (
            "SELECT provider,kind,external_id FROM album_external_identity WHERE album_id=?1",
            album.as_str(),
        ),
        (
            "SELECT i.provider,i.kind,i.external_id FROM release_external_identity i JOIN release r ON r.id=i.release_id WHERE r.album_id=?1",
            album.as_str(),
        ),
    ] {
        identities.extend(
            db.prepare(sql)?
                .query_map([key], |r| {
                    Ok(format!(
                        "{} / {}: {}",
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?,
        );
    }
    let mut fields = Vec::new();
    for (key, label) in specs(target) {
        let overridden = override_value(db, target, key)?.is_some();
        let source = if overridden {
            "User override".to_string()
        } else if let Target::Track(t) = target {
            let inherited = ["artist_credit", "genre", "year"].contains(key)
                && override_value(db, &Target::Album(album.clone()), key)?.is_some()
                && (*key != "artist_credit"
                    || effective.get(*key)
                        == override_value(db, &Target::Album(album.clone()), key)?.as_ref());
            if inherited {
                "Album user override".into()
            } else if *key == "title" || *key == "year" {
                let local:bool=db.query_row("SELECT count(*)=1 AND max(CASE WHEN ?2='title' THEN f.track_title IS NOT NULL ELSE f.year IS NOT NULL END) FROM track_source s LEFT JOIN file_metadata_observation f ON f.source_id=s.source_id WHERE s.track_id=?1",params![t,key],|r|r.get::<_,Option<bool>>(0))?.unwrap_or(false);
                if local {
                    "Local file".into()
                } else {
                    "Application metadata".into()
                }
            } else if *key == "genre" {
                "Local file tags (all attached sources)".into()
            } else {
                "Application metadata".into()
            }
        } else if *key == "release_type" {
            "Trusted provider evidence (agreed, if present)".into()
        } else {
            "Application metadata".into()
        };
        fields.push(Field {
            key: (*key).into(),
            label: (*label).into(),
            value: effective.remove(*key).unwrap_or_default(),
            overridden,
            effective_source: source,
            editable: true,
        });
    }
    if matches!(target, Target::Track(_)) {
        for (key, label) in [
            ("album", "Album (edit shared fields in Album Metadata)"),
            ("duration", "Duration"),
        ] {
            fields.push(Field {
                key: key.into(),
                label: label.into(),
                value: effective.remove(key).unwrap_or_default(),
                overridden: false,
                effective_source: "Automatic".into(),
                editable: false,
            });
        }
        fields.push(Field {
            key: "album_artist".into(),
            label: "Album artist credit".into(),
            value: override_value(db, &Target::Album(album.clone()), "artist_credit")?
                .unwrap_or(credit(db, "album", &album)?),
            overridden: override_value(db, &Target::Album(album.clone()), "artist_credit")?
                .is_some(),
            effective_source: "Shared Album metadata".into(),
            editable: false,
        });
        fields.push(Field {
            key: "release_type".into(),
            label: "Release type".into(),
            value: override_value(db, &Target::Album(album.clone()), "release_type")?
                .or(automatic_release_type(db, &album)?)
                .unwrap_or_default(),
            overridden: false,
            effective_source: "Shared Album metadata / provider evidence below".into(),
            editable: false,
        });
    }
    if let Target::Track(t) = target {
        identities.extend(db.prepare("SELECT provider,kind,external_id FROM track_external_identity WHERE track_id=?1 UNION SELECT i.provider,i.kind,i.external_id FROM recording_external_identity i JOIN track t ON t.recording_id=i.recording_id WHERE t.id=?1")?.query_map([t],|r|Ok(format!("{} / {}: {}",r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    identities.sort();
    identities.dedup();
    let mut track_evidence = Vec::new();
    if matches!(target, Target::Album(_)) {
        let mut grouped = BTreeMap::<String, Vec<Evidence>>::new();
        evidence.retain(|item| {
            if let Some(track) = &item.track_id {
                grouped.entry(track.clone()).or_default().push(item.clone());
                false
            } else {
                true
            }
        });
        let sql = "SELECT e.track_id,e.title,e.disc_number,e.track_number,e.artist_names,e.genre_names,e.year,e.duration_ms,e.release_title,t.release_id FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id JOIN effective_album_metadata a ON a.album_id=r.album_id WHERE r.album_id=?1 ORDER BY COALESCE(e.disc_number,1),COALESCE(e.track_number,2147483647),t.release_id,t.id".replace("e.artist_names", &crate::browse::song_artist_credit_sql());
        let mut query = db.prepare(&sql)?;
        for row in query.query_map([&album], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, Option<i64>>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
            ))
        })? {
            let (track, title, disc, position, artist, genre, year, duration, album_title, release) =
                row?;
            let mut values = BTreeMap::new();
            values.insert("Track title".into(), title.clone());
            values.insert("Artist credit".into(), artist);
            values.insert("Album".into(), album_title);
            add(&mut values, "Genre", Some(genre));
            add(&mut values, "Year", number(year));
            add(&mut values, "Disc number", number(disc));
            add(&mut values, "Track number", number(position));
            add(&mut values, "Duration (ms)", number(duration));
            let mut sources = grouped.remove(&track).unwrap_or_default();
            sources.sort_by(|a, b| a.source.cmp(&b.source).then(a.label.cmp(&b.label)));
            sources.insert(
                0,
                Evidence {
                    candidate_id: None,
                    track_id: Some(track.clone()),
                    source: "Library".into(),
                    label: format!("Effective Library metadata · Release {release}"),
                    values,
                },
            );
            track_evidence.push(TrackEvidence {
                track_id: track,
                title,
                disc_number: disc,
                track_number: position,
                evidence: sources,
            });
        }
    }
    Ok(Inspection {
        target: target.clone(),
        assigned_artist: assigned_artist(db, target)?,
        album_id: album,
        fields,
        evidence,
        track_evidence,
        files,
        track_count: tracks.len(),
        identities,
    })
}
fn automatic_release_type(db: &Connection, album: &str) -> Result<Option<String>> {
    let evidence =
        crate::canonical_evidence::load_album(db, &crate::domain::AlbumId(album.into()))?;
    let values = evidence.release_types;
    Ok(values
        .first()
        .filter(|first| {
            values.iter().all(|v| {
                crate::matching::normalize(&v.value) == crate::matching::normalize(&first.value)
            })
        })
        .map(|v| v.value.clone()))
}
fn provider_name(name: &str) -> String {
    match name {
        "musicbrainz" => "MusicBrainz".into(),
        "spotify" => "Spotify".into(),
        _ => name.into(),
    }
}

fn write_tags(
    path: &std::path::Path,
    target: &Target,
    values: &BTreeMap<String, String>,
) -> std::result::Result<(), String> {
    use lofty::prelude::Accessor;
    use lofty::{
        config::WriteOptions,
        file::{AudioFile, FileType, TaggedFileExt},
        tag::{ItemKey, Tag},
    };
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("Only regular local files can be updated".into());
    }
    if meta.permissions().readonly() {
        return Err("File is read-only".into());
    }
    let mut audio = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    if !matches!(
        audio.file_type(),
        FileType::Mpeg | FileType::Flac | FileType::Mp4 | FileType::Vorbis | FileType::Opus
    ) {
        return Err("Tag write-back supports MP3, FLAC, M4A/MP4, Ogg Vorbis and Opus".into());
    }
    if values.contains_key("release_type") {
        return Err("Release type has no supported round-trip tag mapping".into());
    }
    let tag_type = audio.primary_tag_type();
    if audio.primary_tag().is_none() {
        audio.insert_tag(Tag::new(tag_type));
    }
    let tag = audio.primary_tag_mut().ok_or("No writable primary tag")?;
    for (field, value) in values {
        match field.as_str() {
            "title" => {
                if matches!(target, Target::Album(_)) {
                    if value.is_empty() {
                        tag.remove_album();
                    } else {
                        tag.set_album(value.clone());
                    }
                } else {
                    tag.set_title(value.clone());
                }
            }
            "artist_assignment" | "artist_assignment_album_only" => {
                let keys = if matches!(target, Target::Album(_)) {
                    if field == "artist_assignment_album_only" {
                        vec![ItemKey::AlbumArtist]
                    } else {
                        vec![ItemKey::AlbumArtist, ItemKey::TrackArtist]
                    }
                } else {
                    vec![ItemKey::TrackArtist]
                };
                for key in keys {
                    tag.remove_key(key);
                    if !tag.insert_text(key, value.clone()) {
                        return Err("Artist tag unsupported".into());
                    }
                }
            }
            "artist_credit" => {
                let key = if matches!(target, Target::Album(_)) {
                    ItemKey::AlbumArtist
                } else {
                    ItemKey::TrackArtist
                };
                tag.remove_key(key);
                if !value.is_empty() && !tag.insert_text(key, value.clone()) {
                    return Err("Artist credit not supported by this tag".into());
                }
            }
            "genre" => {
                tag.remove_genre();
                if !value.is_empty() {
                    tag.set_genre(value.clone());
                }
            }
            "year" => {
                tag.remove_date();
                if !value.is_empty() && !tag.insert_text(ItemKey::RecordingDate, value.clone()) {
                    return Err("Date tag unsupported".into());
                }
            }
            "disc_number" => {
                tag.remove_disk();
                if !value.is_empty() {
                    tag.set_disk(value.parse().map_err(|_| "Invalid disc")?);
                }
            }
            "track_number" => {
                tag.remove_track();
                if !value.is_empty() {
                    tag.set_track(value.parse().map_err(|_| "Invalid track number")?);
                }
            }
            _ => return Err(format!("Unsupported tag field {field}")),
        }
    }
    // Lofty writes to a complete sibling copy; the original remains intact on failure.
    // Atomic rename preserves the path/source identity. Refuse competing file changes.
    let mut temp = tempfile::Builder::new()
        .prefix(".metadata-")
        // Scanner adapters must not discover the in-progress media copy.
        .suffix(".tmp")
        .tempfile_in(path.parent().ok_or("No parent directory")?)
        .map_err(|e| e.to_string())?;
    std::io::copy(
        &mut std::fs::File::open(path).map_err(|e| e.to_string())?,
        temp.as_file_mut(),
    )
    .map_err(|e| e.to_string())?;
    audio
        .save_to(temp.as_file_mut(), WriteOptions::default())
        .map_err(|e| e.to_string())?;
    temp.as_file()
        .set_permissions(meta.permissions())
        .map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    // Validate using the normal reader before replacing the file.
    lofty::probe::Probe::open(temp.path())
        .map_err(|e| e.to_string())?
        .set_file_type(audio.file_type())
        .read()
        .map_err(|e| format!("Written file failed verification: {e}"))?;
    let current = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if current.len() != meta.len() || current.modified().ok() != meta.modified().ok() {
        return Err("File changed while tags were being written".into());
    }
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
