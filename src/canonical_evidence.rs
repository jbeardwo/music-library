//! Indexed persisted evidence for reconciliation. Observations never rewrite
//! display metadata, credits, sources, or Library membership.
use crate::{
    domain::*,
    storage::{Error, Result},
};
use rusqlite::{Connection, params};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum Origin {
    Application,
    UserOverride,
    Local { source_id: String },
    Provider { identity: ExternalIdentity },
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Observation<T> {
    pub value: T,
    pub origin: Origin,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TrackEvidence {
    pub titles: Vec<Observation<String>>,
    pub artist_credits: Vec<Observation<Vec<String>>>,
    pub album_titles: Vec<Observation<String>>,
    pub dates: Vec<Observation<String>>,
    pub release_types: Vec<Observation<String>>,
    pub discs: Vec<Observation<u32>>,
    pub positions: Vec<Observation<u32>>,
    pub durations: Vec<Observation<u64>>,
}
impl TrackEvidence {
    pub fn annotations(&self) -> String {
        let mut rows = Vec::new();
        for (label, values) in [
            ("Song", &self.titles),
            ("Album", &self.album_titles),
            ("Release date", &self.dates),
            ("Release type", &self.release_types),
        ] {
            for o in values {
                rows.push(format!(
                    "{label}: {} ({})",
                    o.value,
                    match &o.origin {
                        Origin::Application => "application".into(),
                        Origin::UserOverride => "user override".into(),
                        Origin::Local { source_id } => format!("local source {source_id}"),
                        Origin::Provider { identity } => format!(
                            "{} {} {}",
                            identity.provider, identity.kind, identity.external_id
                        ),
                    }
                ));
            }
        }
        for o in &self.artist_credits {
            rows.push(format!(
                "Artist credit: {} ({:?})",
                o.value.join(", "),
                o.origin
            ));
        }
        for observation in &self.discs {
            rows.push(format!(
                "Disc: {} ({:?})",
                observation.value, observation.origin
            ));
        }
        for observation in &self.positions {
            rows.push(format!(
                "Track: {} ({:?})",
                observation.value, observation.origin
            ));
        }
        for observation in &self.durations {
            rows.push(format!(
                "Duration: {} ms ({:?})",
                observation.value, observation.origin
            ));
        }
        rows.join("\n")
    }
}
pub(crate) fn observe_album(
    db: &Connection,
    album: &AlbumId,
    id: &ExternalIdentity,
    title: &str,
    date: &str,
    release_type: Option<&str>,
) -> Result<()> {
    if id.external_id.is_empty() {
        return Err(Error::Invalid(
            "Evidence requires a provider identity".into(),
        ));
    }
    db.execute("INSERT INTO album_provider_evidence(album_id,provider,kind,external_id,title,release_date,release_type) VALUES(?1,?2,?3,?4,?5,NULLIF(?6,''),?7) ON CONFLICT(album_id,provider,kind,external_id) DO UPDATE SET title=excluded.title,release_date=excluded.release_date,release_type=COALESCE(excluded.release_type,album_provider_evidence.release_type)", params![album.as_ref(),id.provider,id.kind,id.external_id,title,date,release_type])?;
    Ok(())
}
pub(crate) fn observe_track(
    db: &Connection,
    track: &TrackId,
    id: &ExternalIdentity,
    title: &str,
    disc: u32,
    position: u32,
    duration: Option<u64>,
) -> Result<()> {
    db.execute("INSERT INTO track_provider_evidence(track_id,provider,kind,external_id,title,disc,position,duration_ms) VALUES(?1,?2,?3,?4,?5,NULLIF(?6,0),NULLIF(?7,0),?8) ON CONFLICT(track_id,provider,kind,external_id) DO UPDATE SET title=excluded.title,disc=excluded.disc,position=excluded.position,duration_ms=excluded.duration_ms",params![track.as_ref(),id.provider,id.kind,id.external_id,title,disc,position,duration.map(|d|d as i64)])?;
    Ok(())
}
/// All reads are scoped to one canonical Track and its Album. Unavailable local
/// observations remain evidence; no filesystem or provider requests occur here.
pub(crate) fn load(db: &Connection, track: &TrackId, album: &str) -> Result<TrackEvidence> {
    let mut e = TrackEvidence::default();
    let (title, album_title, year):(String,String,Option<i32>)=db.query_row("SELECT m.title,a.title,a.year FROM track t JOIN track_application_metadata m ON m.track_id=t.id JOIN release r ON r.id=t.release_id JOIN album_application_metadata a ON a.album_id=r.album_id WHERE t.id=?1",[track.as_ref()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    if !title.is_empty() {
        e.titles.push(Observation {
            value: title,
            origin: Origin::Application,
        });
    }
    if !album_title.is_empty() {
        e.album_titles.push(Observation {
            value: album_title,
            origin: Origin::Application,
        });
    }
    if let Some(y) = year {
        e.dates.push(Observation {
            value: y.to_string(),
            origin: Origin::Application,
        });
    }
    let mut q = db.prepare("SELECT value FROM track_title_override WHERE track_id=?1")?;
    for value in q.query_map([track.as_ref()], |r| r.get::<_, String>(0))? {
        e.titles.push(Observation {
            value: value?,
            origin: Origin::UserOverride,
        });
    }
    let mut q=db.prepare("SELECT ts.source_id,f.track_title,f.release_title,f.year,f.disc_number,f.track_number,f.duration_ms FROM track_source ts JOIN file_metadata_observation f ON f.source_id=ts.source_id WHERE ts.track_id=?1")?;
    for r in q.query_map([track.as_ref()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<i32>>(3)?,
            r.get::<_, Option<u32>>(4)?,
            r.get::<_, Option<u32>>(5)?,
            r.get::<_, Option<i64>>(6)?
                .and_then(|v| u64::try_from(v).ok()),
        ))
    })? {
        let (id, title, album, year, disc, position, duration) = r?;
        let origin = Origin::Local { source_id: id };
        if let Some(value) = title.filter(|v| !v.trim().is_empty()) {
            e.titles.push(Observation {
                value,
                origin: origin.clone(),
            });
        }
        if let Some(value) = album.filter(|v| !v.trim().is_empty()) {
            e.album_titles.push(Observation {
                value,
                origin: origin.clone(),
            });
        }
        if let Some(y) = year {
            e.dates.push(Observation {
                value: y.to_string(),
                origin: origin.clone(),
            });
        }
        if let Some(value) = disc.filter(|v| *v > 0) {
            e.discs.push(Observation {
                value,
                origin: origin.clone(),
            });
        }
        if let Some(value) = position.filter(|v| *v > 0) {
            e.positions.push(Observation {
                value,
                origin: origin.clone(),
            });
        }
        if let Some(value) = duration.filter(|v| *v > 0) {
            e.durations.push(Observation { value, origin });
        }
    }
    let mut credits = std::collections::BTreeMap::<String, Vec<String>>::new();
    let mut q=db.prepare("SELECT ts.source_id,f.name FROM track_source ts JOIN file_artist_observation f ON f.source_id=ts.source_id AND f.scope='track' WHERE ts.track_id=?1 ORDER BY ts.source_id,f.position")?;
    for row in q.query_map([track.as_ref()], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (source, name) = row?;
        credits.entry(source).or_default().push(name);
    }
    for (source_id, value) in credits {
        e.artist_credits.push(Observation {
            value,
            origin: Origin::Local { source_id },
        });
    }
    let mut q=db.prepare("SELECT provider,kind,external_id,title,release_date,release_type FROM album_provider_evidence e WHERE album_id=?1 AND EXISTS(SELECT 1 FROM album_external_identity i WHERE i.album_id=e.album_id AND i.provider=e.provider AND i.kind=e.kind AND i.external_id=e.external_id) ORDER BY provider,kind,external_id")?;
    for r in q.query_map([album], |r| {
        Ok((
            ExternalIdentity {
                provider: r.get(0)?,
                kind: r.get(1)?,
                external_id: r.get(2)?,
            },
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
        ))
    })? {
        let (id, title, date, kind) = r?;
        let origin = Origin::Provider { identity: id };
        e.album_titles.push(Observation {
            value: title,
            origin: origin.clone(),
        });
        if let Some(value) = date {
            e.dates.push(Observation {
                value,
                origin: origin.clone(),
            });
        }
        if let Some(value) = kind.filter(|v| !v.is_empty()) {
            e.release_types.push(Observation { value, origin });
        }
    }
    let mut q=db.prepare("SELECT provider,kind,external_id,title,disc,position,duration_ms FROM track_provider_evidence e WHERE track_id=?1 AND EXISTS(SELECT 1 FROM track t JOIN release_external_identity i ON i.release_id=t.release_id WHERE t.id=e.track_id AND i.provider=e.provider AND i.kind=e.kind AND i.external_id=e.external_id) ORDER BY provider,kind,external_id")?;
    for r in q.query_map([track.as_ref()], |r| {
        Ok((
            ExternalIdentity {
                provider: r.get(0)?,
                kind: r.get(1)?,
                external_id: r.get(2)?,
            },
            r.get::<_, String>(3)?,
            r.get::<_, Option<u32>>(4)?,
            r.get::<_, Option<u32>>(5)?,
            r.get::<_, Option<i64>>(6)?
                .and_then(|v| u64::try_from(v).ok()),
        ))
    })? {
        let (id, title, disc, position, duration) = r?;
        let origin = Origin::Provider { identity: id };
        e.titles.push(Observation {
            value: title,
            origin: origin.clone(),
        });
        if let Some(value) = disc {
            e.discs.push(Observation {
                value,
                origin: origin.clone(),
            });
        }
        if let Some(value) = position {
            e.positions.push(Observation {
                value,
                origin: origin.clone(),
            });
        }
        if let Some(value) = duration {
            e.durations.push(Observation { value, origin });
        }
    }
    Ok(e)
}

/// One Album-scoped query for all persisted catalog titles. The program evaluator
/// consults these observations without one lookup per Track.
pub(crate) fn load_program_titles(
    db: &Connection,
    tracks: &mut [crate::edition::LocalTrackEvidence],
    album: &AlbumId,
) -> Result<()> {
    let mut indices = std::collections::HashMap::new();
    for (i, t) in tracks.iter().enumerate() {
        indices.insert(t.track_id.as_ref().to_owned(), i);
    }
    let mut q=db.prepare("SELECT e.track_id,e.provider,e.kind,e.external_id,e.title,e.disc,e.position,e.duration_ms,NULL FROM release r JOIN track t ON t.release_id=r.id JOIN track_provider_evidence e ON e.track_id=t.id JOIN release_external_identity i ON i.release_id=r.id AND i.provider=e.provider AND i.kind=e.kind AND i.external_id=e.external_id WHERE r.album_id=?1 UNION ALL SELECT ts.track_id,'','','',f.track_title,f.disc_number,f.track_number,f.duration_ms,ts.source_id FROM release r JOIN track t ON t.release_id=r.id JOIN track_source ts ON ts.track_id=t.id JOIN file_metadata_observation f ON f.source_id=ts.source_id WHERE r.album_id=?1 ORDER BY 1,2,3,4")?;
    for row in q.query_map([album.as_ref()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            ExternalIdentity {
                provider: r.get(1)?,
                kind: r.get(2)?,
                external_id: r.get(3)?,
            },
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<u32>>(5)?,
            r.get::<_, Option<u32>>(6)?,
            r.get::<_, Option<i64>>(7)?
                .and_then(|v| u64::try_from(v).ok()),
            r.get::<_, Option<String>>(8)?,
        ))
    })? {
        let (track, identity, title, disc, number, duration, source) = row?;
        if let Some(i) = indices.get(&track) {
            let known = &mut tracks[*i].evidence;
            let origin = source.map_or(Origin::Provider { identity }, |source_id| Origin::Local {
                source_id,
            });
            known.disc = known.disc.or(disc.filter(|v| *v > 0));
            known.number = known.number.or(number.filter(|v| *v > 0));
            known.duration_ms = known.duration_ms.or(duration);
            known.position_observations.push(Observation {
                value: Position { disc, number },
                origin: origin.clone(),
            });
            if let Some(value) = title.filter(|v| !v.is_empty()) {
                known.title_observations.push(Observation { value, origin });
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct AlbumEvidence {
    pub titles: Vec<Observation<String>>,
    pub dates: Vec<Observation<String>>,
    pub release_types: Vec<Observation<String>>,
}
pub(crate) fn load_album(db: &Connection, album: &AlbumId) -> Result<AlbumEvidence> {
    let mut evidence = AlbumEvidence::default();
    let mut q=db.prepare("SELECT provider,kind,external_id,title,release_date,release_type FROM album_provider_evidence e WHERE album_id=?1 AND EXISTS(SELECT 1 FROM album_external_identity i WHERE i.album_id=e.album_id AND i.provider=e.provider AND i.kind=e.kind AND i.external_id=e.external_id) ORDER BY provider,kind,external_id")?;
    for row in q.query_map([album.as_ref()], |r| {
        Ok((
            ExternalIdentity {
                provider: r.get(0)?,
                kind: r.get(1)?,
                external_id: r.get(2)?,
            },
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
        ))
    })? {
        let (identity, value, date, kind) = row?;
        let origin = Origin::Provider { identity };
        evidence.titles.push(Observation {
            value,
            origin: origin.clone(),
        });
        if let Some(value) = date {
            evidence.dates.push(Observation {
                value,
                origin: origin.clone(),
            });
        }
        if let Some(value) = kind.filter(|v| !v.is_empty()) {
            evidence.release_types.push(Observation { value, origin });
        }
    }
    Ok(evidence)
}

pub fn release_type_compatible(known: &str, candidate: &str, provider: &str) -> bool {
    let a = crate::matching::normalize(known);
    let b = crate::matching::normalize(candidate);
    a.is_empty() || b.is_empty() || a == b || provider == "spotify" && a == "ep" && b == "single"
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Position {
    pub disc: Option<u32>,
    pub number: Option<u32>,
}

/// Accumulate compatible precision rather than privileging one contributor.
/// Conflicting years withhold date ranking; conflicting finer precision retains
/// only the common year/month. Raw observations remain available for diagnostics.
pub fn agreed_date(values: &[Observation<String>]) -> Option<crate::catalog_date::Date> {
    let dates: Vec<_> = values
        .iter()
        .filter_map(|o| crate::catalog_date::Date::parse(&o.value))
        .collect();
    let first = dates.first()?;
    if dates.iter().any(|d| d.year != first.year) {
        return None;
    }
    let month = dates
        .iter()
        .find_map(|d| d.month)
        .filter(|month| dates.iter().all(|d| d.month.is_none_or(|m| m == *month)));
    let day = month.and_then(|_| {
        dates
            .iter()
            .find_map(|d| d.day)
            .filter(|day| dates.iter().all(|d| d.day.is_none_or(|d| d == *day)))
    });
    Some(crate::catalog_date::Date {
        year: first.year,
        month,
        day,
    })
}
