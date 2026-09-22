//! Explicit-Play route selection. No network, identity mutation, or queue advancement.
use crate::{
    domain::*,
    storage::{Result, Store},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ActiveBackend {
    #[default]
    None,
    Local,
    Remote(String),
}

/// A playback adapter supplies capability separately from catalog identity.
pub struct RemoteCapability<'a> {
    pub provider: &'a str,
    pub unavailable: Option<String>,
    pub catalog_available: bool,
    pub accepts: fn(&ExternalIdentity) -> bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Local(PlayableSource),
    Remote(ExternalIdentity),
    NeedsEnrichment,
    Unavailable(String),
}

impl Store {
    /// Indexed, Track-scoped lookup; filesystem probes are outside transactions.
    /// Missing sources remain untouched. Unexpected access errors do not masquerade
    /// as absence and do not cause a remote fallback.
    pub fn playback_route(&self, track: &TrackId, remote: &RemoteCapability<'_>) -> Result<Route> {
        for source in self.playback_sources(track)? {
            let SourceLocation::LocalFile(path) = &source.location;
            match std::fs::metadata(path) {
                Ok(metadata) if metadata.is_file() => {}
                Ok(_) => continue,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                    ) =>
                {
                    continue;
                }
                Err(error) => {
                    return Ok(Route::Unavailable(format!(
                        "Local source check failed: {error}"
                    )));
                }
            }
            match std::fs::File::open(path) {
                Ok(file) => match file.metadata() {
                    Ok(metadata) if metadata.is_file() => return Ok(Route::Local(source)),
                    Ok(_) => continue,
                    Err(error) => {
                        return Ok(Route::Unavailable(format!(
                            "Local source check failed: {error}"
                        )));
                    }
                },
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                    ) =>
                {
                    continue;
                }
                Err(error) => {
                    return Ok(Route::Unavailable(format!(
                        "Local source cannot be opened: {error}"
                    )));
                }
            }
        }
        if let Some(reason) = &remote.unavailable {
            return Ok(Route::Unavailable(format!(
                "No usable local source; {reason}"
            )));
        }
        let identities = self.track_provider_occurrences(track, remote.provider)?;
        let valid: Vec<_> = identities
            .iter()
            .filter(|id| (remote.accepts)(id))
            .collect();
        if let [identity] = valid.as_slice() {
            return Ok(Route::Remote((*identity).clone()));
        }
        if !identities.is_empty() {
            return Ok(Route::Unavailable("Existing provider association is invalid or ambiguous; explicit correction required".into()));
        }
        Ok(if remote.catalog_available {
            Route::NeedsEnrichment
        } else {
            Route::Unavailable("No playable source; remote catalog is not configured".into())
        })
    }
}

impl crate::storage::Store {
    pub(crate) fn playback_sources(&self, track: &TrackId) -> Result<Vec<PlayableSource>> {
        let mut query = self.connection.prepare("SELECT ps.id,l.path FROM track_source ts CROSS JOIN playable_source ps ON ps.id=ts.source_id CROSS JOIN local_file_observation l ON l.source_id=ps.id WHERE ts.track_id=?1 AND ps.kind='local_file' AND l.available=1 ORDER BY ts.source_id COLLATE BINARY")?;
        Ok(query
            .query_map([track.as_ref()], |row| {
                Ok(PlayableSource {
                    source_id: SourceId(row.get(0)?),
                    location: SourceLocation::LocalFile(crate::storage::bytes_to_path(row.get(1)?)),
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
}
