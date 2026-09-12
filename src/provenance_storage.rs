//! Storage codec for source observations, never canonical identity lookups.
use crate::{
    domain::SourceId,
    provenance::FileProvenance,
    storage::{Error, Result},
};

#[derive(serde::Serialize, serde::Deserialize)]
struct Snapshot<T> {
    version: u32,
    observation: T,
}

/// Serialize already extracted values before starting the scan write transaction.
pub(crate) fn encode(value: &FileProvenance) -> Result<String> {
    serde_json::to_string(&Snapshot {
        version: 1,
        observation: value,
    })
    .map_err(|e| Error::Invalid(format!("cannot encode source provenance: {e}")))
}

pub(crate) fn decode(source: &SourceId, json: Option<&str>) -> Result<FileProvenance> {
    let mut value = if let Some(json) = json {
        let snapshot: Snapshot<FileProvenance> = serde_json::from_str(json).map_err(|e| {
            Error::Invalid(format!("invalid provenance for source {}: {e}", source.0))
        })?;
        if snapshot.version != 1 {
            return Err(Error::Invalid(format!(
                "unsupported provenance version {} for source {}",
                snapshot.version, source.0
            )));
        }
        snapshot.observation
    } else {
        FileProvenance::default()
    };
    value.source_id = Some(source.clone());
    Ok(value)
}
