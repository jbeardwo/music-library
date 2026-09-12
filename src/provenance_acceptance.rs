//! Local claims are evaluated separately from accepted identities. No provider syntax here.
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    domain::ExternalIdentity,
    provenance::{Observation, Origin, Scope, Semantics},
};

/// Adapter capability and validation result. Invalid claims block their namespace,
/// rather than disappearing from conflict detection. Unsupported claims remain raw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validation {
    Unsupported,
    Invalid,
    Valid,
}
pub type Validator = fn(&Observation) -> Validation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Accepted,
    Invalid,
    ConflictingObservations,
    ConflictingIndependentIdentity,
}

#[derive(Clone, Debug)]
pub struct Assessment {
    pub provider: String,
    pub kind: String,
    pub values: Vec<String>,
    pub decision: Decision,
}

/// Independent canonical associations constrain acceptance; managed ones are
/// reevaluated from scratch. Completeness, Artist credits and editions are irrelevant.
pub fn assess(
    observations: &[Observation],
    independent: &[ExternalIdentity],
    validator: Validator,
) -> Vec<Assessment> {
    let mut groups = BTreeMap::<(String, String), (BTreeSet<String>, bool)>::new();
    for o in observations {
        if !matches!(o.origin, Origin::EmbeddedTag { .. })
            || !matches!(
                (o.scope, o.semantics),
                (Scope::Album, Semantics::AlbumIdentity)
                    | (Scope::Recording, Semantics::RecordingIdentity)
            )
        {
            continue;
        }
        let validity = validator(o);
        if validity == Validation::Unsupported {
            continue;
        }
        let group = groups
            .entry((o.identity.provider.clone(), o.identity.kind.clone()))
            .or_default();
        group.0.insert(o.identity.external_id.clone());
        group.1 |= validity == Validation::Invalid;
    }
    groups
        .into_iter()
        .map(|((provider, kind), (values, invalid))| {
            let values: Vec<_> = values.into_iter().collect();
            let decision = if values.len() != 1 {
                Decision::ConflictingObservations
            } else if invalid {
                Decision::Invalid
            } else if independent
                .iter()
                .any(|i| i.provider == provider && i.kind == kind && i.external_id != values[0])
            {
                Decision::ConflictingIndependentIdentity
            } else {
                Decision::Accepted
            };
            Assessment {
                provider,
                kind,
                values,
                decision,
            }
        })
        .collect()
}

use crate::storage::{Result, Store};
use rusqlite::{Connection, params};

#[derive(Clone, Debug)]
pub struct EntityReport {
    pub entity: &'static str,
    pub id: String,
    pub observations: Vec<Observation>,
    pub non_promotable: Vec<Observation>,
    pub assessments: Vec<Assessment>,
    pub independent: Vec<ExternalIdentity>,
    pub managed: Vec<ExternalIdentity>,
}

fn json(ids: &[String]) -> Result<String> {
    serde_json::to_string(ids).map_err(|e| crate::storage::Error::Invalid(e.to_string()))
}

/// All reads are bounded to affected Albums and their Recordings. Shared
/// Recordings include sources belonging to other Albums, without merging Tracks.
pub(crate) fn reconcile(
    db: &Connection,
    albums: &[String],
    validator: Validator,
    write: bool,
) -> Result<Vec<EntityReport>> {
    if albums.is_empty() {
        return Ok(vec![]);
    }
    let album_json = json(albums)?;
    let recordings = db.prepare("SELECT DISTINCT t.recording_id FROM release r CROSS JOIN track t ON t.release_id=r.id WHERE r.album_id IN (SELECT value FROM json_each(?1))")?
        .query_map([&album_json], |r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut reports = vec![];
    for (entity, ids, query, scope, semantics) in [
        (
            "album",
            albums,
            "SELECT r.album_id,ts.source_id,m.provenance_json FROM release r CROSS JOIN track t ON t.release_id=r.id CROSS JOIN track_source ts ON ts.track_id=t.id CROSS JOIN local_file_observation l ON l.source_id=ts.source_id AND l.available=1 LEFT JOIN file_metadata_observation m ON m.source_id=ts.source_id WHERE r.album_id IN (SELECT value FROM json_each(?1))",
            Scope::Album,
            Semantics::AlbumIdentity,
        ),
        (
            "recording",
            recordings.as_slice(),
            "SELECT t.recording_id,ts.source_id,m.provenance_json FROM track t CROSS JOIN track_source ts ON ts.track_id=t.id CROSS JOIN local_file_observation l ON l.source_id=ts.source_id AND l.available=1 LEFT JOIN file_metadata_observation m ON m.source_id=ts.source_id WHERE t.recording_id IN (SELECT value FROM json_each(?1))",
            Scope::Recording,
            Semantics::RecordingIdentity,
        ),
    ] {
        let ids_json = json(ids)?;
        let mut entities: BTreeMap<String, EntityReport> = ids
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    EntityReport {
                        entity,
                        id: id.clone(),
                        observations: vec![],
                        non_promotable: vec![],
                        assessments: vec![],
                        independent: vec![],
                        managed: vec![],
                    },
                )
            })
            .collect();
        let mut q = db.prepare(query)?;
        let rows = q.query_map([&ids_json], |r| {
            Ok((
                r.get::<_, String>(0)?,
                crate::domain::SourceId(r.get(1)?),
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        for row in rows {
            let (id, source, raw) = row?;
            let file = crate::provenance_storage::decode(&source, raw.as_deref())?;
            entities
                .get_mut(&id)
                .expect("scoped entity")
                .observations
                .extend(
                    file.observations
                        .into_iter()
                        .filter(|o| o.scope == scope && o.semantics == semantics),
                );
        }
        let table = format!("{entity}_external_identity");
        let marker = format!("{entity}_provenance_identity");
        let key = format!("{entity}_id");
        let sql = format!(
            "SELECT i.{key},i.provider,i.kind,i.external_id,m.{key} IS NOT NULL FROM {table} i LEFT JOIN {marker} m USING({key},provider,kind,external_id) WHERE i.{key} IN (SELECT value FROM json_each(?1))"
        );
        for row in db.prepare(&sql)?.query_map([&ids_json], |r| {
            Ok((
                r.get::<_, String>(0)?,
                ExternalIdentity {
                    provider: r.get(1)?,
                    kind: r.get(2)?,
                    external_id: r.get(3)?,
                },
                r.get::<_, bool>(4)?,
            ))
        })? {
            let (id, identity, managed) = row?;
            let report = entities.get_mut(&id).expect("scoped identity");
            if managed {
                report.managed.push(identity);
            } else {
                report.independent.push(identity);
            }
        }
        let mut delete = db.prepare(&format!(
            "DELETE FROM {table} WHERE {key}=?1 AND provider=?2 AND kind=?3 AND external_id=?4"
        ))?;
        let mut insert = db.prepare(&format!(
            "INSERT INTO {table}({key},provider,kind,external_id) VALUES (?1,?2,?3,?4)"
        ))?;
        let mut mark = db.prepare(&format!(
            "INSERT INTO {marker}({key},provider,kind,external_id) VALUES (?1,?2,?3,?4)"
        ))?;
        for mut report in entities.into_values() {
            report.non_promotable = report
                .observations
                .iter()
                .filter(|o| {
                    !matches!(o.origin, Origin::EmbeddedTag { .. })
                        || validator(o) == Validation::Unsupported
                })
                .cloned()
                .collect();
            report.assessments = assess(&report.observations, &report.independent, validator);
            let desired: Vec<_> = report
                .assessments
                .iter()
                .filter(|a| a.decision == Decision::Accepted)
                .map(|a| ExternalIdentity {
                    provider: a.provider.clone(),
                    kind: a.kind.clone(),
                    external_id: a.values[0].clone(),
                })
                .filter(|i| !report.independent.contains(i))
                .collect();
            if write {
                for i in report.managed.iter().filter(|i| !desired.contains(i)) {
                    delete.execute(params![report.id, i.provider, i.kind, i.external_id])?;
                }
                for i in desired.iter().filter(|i| !report.managed.contains(i)) {
                    insert.execute(params![report.id, i.provider, i.kind, i.external_id])?;
                    mark.execute(params![report.id, i.provider, i.kind, i.external_id])?;
                }
                report.managed = desired;
            }
            reports.push(report);
        }
    }
    Ok(reports)
}

pub(crate) fn albums_for_sources(db: &Connection, sources: &[String]) -> Result<Vec<String>> {
    if sources.is_empty() {
        return Ok(vec![]);
    }
    Ok(db.prepare("SELECT DISTINCT r.album_id FROM track_source ts JOIN track t ON t.id=ts.track_id JOIN release r ON r.id=t.release_id WHERE ts.source_id IN (SELECT value FROM json_each(?1))")?
        .query_map([json(sources)?], |r|r.get(0))?.collect::<rusqlite::Result<_>>()?)
}

impl Store {
    pub fn reconcile_local_provenance(
        &mut self,
        album: &crate::domain::AlbumId,
    ) -> Result<Vec<EntityReport>> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let reports = reconcile(
            &tx,
            &[album.as_ref().to_owned()],
            self.provenance_validator,
            true,
        )?;
        tx.commit()?;
        Ok(reports)
    }
}

/// Diagnostic-only snapshot: no migrations, filesystem reads, or identity writes.
pub fn inspect(
    db: &Connection,
    album: &crate::domain::AlbumId,
    validator: Validator,
) -> Result<Vec<EntityReport>> {
    reconcile(db, &[album.as_ref().to_owned()], validator, false)
}

/// Uses the standard embedded-file capability mapping without exposing provider
/// details to diagnostic callers. Opens no files other than the database.
pub fn inspect_database(
    path: impl AsRef<std::path::Path>,
    album: &crate::domain::AlbumId,
) -> Result<Vec<EntityReport>> {
    let mut db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let tx = db.transaction()?;
    let result = inspect(&tx, album, crate::filesystem::provenance::validate)?;
    tx.commit()?;
    Ok(result)
}
