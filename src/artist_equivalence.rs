//! Explicit identity equivalence; no name inference, credit edits or Artist merge.
use crate::{
    Library,
    domain::{ArtistId, ExternalIdentity, TrackId},
    edition::ArtistEvidence,
    storage::{Error, Result},
};
use rusqlite::params;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub track: TrackId,
    pub local: ArtistId,
    pub local_name: String,
    pub local_identities: Vec<ExternalIdentity>,
    pub candidate_name: String,
    pub candidate_identity: ExternalIdentity,
}
/// Indexed traversal starts only at the credited Artists requested by the caller.
/// The returned IDs are derived matching evidence, never stored on a different Artist.
pub(crate) fn identities(
    db: &rusqlite::Connection,
    artists: &[String],
) -> Result<HashMap<String, Vec<ExternalIdentity>>> {
    let seeds = serde_json::to_string(artists).map_err(|e| Error::Invalid(e.to_string()))?;
    let mut result = HashMap::<String, Vec<ExternalIdentity>>::new();
    for row in db.prepare("WITH RECURSIVE peers(root,id) AS (SELECT value,value FROM json_each(?1) UNION SELECT p.root,e.artist_b FROM peers p JOIN artist_equivalence e ON e.artist_a=p.id UNION SELECT p.root,e.artist_a FROM peers p JOIN artist_equivalence e ON e.artist_b=p.id) SELECT DISTINCT p.root,i.provider,i.kind,i.external_id FROM peers p JOIN artist_external_identity i ON i.artist_id=p.id WHERE i.kind='artist' ORDER BY p.root,i.provider,i.kind,i.external_id")?.query_map([seeds],|r|Ok((r.get::<_,String>(0)?,ExternalIdentity{provider:r.get(1)?,kind:r.get(2)?,external_id:r.get(3)?})))? {
        let (root,id)=row?;result.entry(root).or_default().push(id);
    }
    Ok(result)
}
pub(crate) fn confirmed_compatible(
    db: &rusqlite::Connection,
    artist: &ArtistId,
    id: &ExternalIdentity,
) -> Result<bool> {
    let linked: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM artist_equivalence WHERE artist_a=?1 OR artist_b=?1)",
        [artist.as_ref()],
        |r| r.get(0),
    )?;
    if !linked {
        return Ok(false);
    }
    Ok(identities(db, std::slice::from_ref(&artist.0))?
        .get(artist.as_ref())
        .is_some_and(|v| v.contains(id)))
}
fn prepare(
    db: &rusqlite::Connection,
    track: &TrackId,
    candidate: &ArtistEvidence,
) -> Result<Proposal> {
    use rusqlite::OptionalExtension;
    let [identity] = candidate.identities.as_slice() else {
        return Err(Error::Invalid(
            "Select one explicitly identified Spotify Artist".into(),
        ));
    };
    if identity.provider != "spotify"
        || identity.kind != "artist"
        || identity.external_id.is_empty()
        || candidate.name.trim().is_empty()
    {
        return Err(Error::Invalid(
            "An identified Spotify Artist is required".into(),
        ));
    }
    let local:Option<String>=db.query_row("SELECT artist_id FROM track_artist_credit WHERE track_id=?1 AND position=(SELECT min(position) FROM track_artist_credit WHERE track_id=?1)",[track.as_ref()],|r|r.get(0)).optional()?;
    let local=local.or(db.query_row("SELECT c.artist_id FROM track t JOIN release r ON r.id=t.release_id JOIN album_artist_credit c ON c.album_id=r.album_id WHERE t.id=?1 ORDER BY c.position LIMIT 1",[track.as_ref()],|r|r.get(0)).optional()?).ok_or_else(||Error::Invalid("No canonical local Artist credit is available".into()))?;
    let name = db.query_row("SELECT name FROM artist WHERE id=?1", [&local], |r| {
        r.get(0)
    })?;
    let id = ArtistId(local);
    Ok(Proposal{track:track.clone(),local_name:name,local_identities:db.prepare("SELECT provider,kind,external_id FROM artist_external_identity WHERE artist_id=?1 ORDER BY provider,kind,external_id")?.query_map([id.as_ref()],|r|Ok(ExternalIdentity{provider:r.get(0)?,kind:r.get(1)?,external_id:r.get(2)?}))?.collect::<rusqlite::Result<Vec<_>>>()?,local:id,candidate_name:candidate.name.clone(),candidate_identity:identity.clone()})
}
impl Library {
    pub fn prepare_artist_equivalence(
        &self,
        track: &TrackId,
        candidate: &ArtistEvidence,
    ) -> Result<Proposal> {
        prepare(&self.store.connection, track, candidate)
    }
    /// Called only after explicit confirmation of the displayed proposal.
    pub fn confirm_artist_equivalence(&mut self, proposal: &Proposal) -> Result<ArtistId> {
        let tx = self
            .store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current = prepare(
            &tx,
            &proposal.track,
            &ArtistEvidence {
                name: proposal.candidate_name.clone(),
                identities: vec![proposal.candidate_identity.clone()],
                join_phrase: String::new(),
            },
        )?;
        if &current != proposal {
            return Err(Error::Invalid(
                "Artist identity changed; review the relationship again".into(),
            ));
        }
        let owners=tx.prepare("SELECT artist_id FROM artist_external_identity WHERE provider=?1 AND kind=?2 AND external_id=?3 ORDER BY artist_id LIMIT 2")?.query_map(params![proposal.candidate_identity.provider,proposal.candidate_identity.kind,proposal.candidate_identity.external_id],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let other =
            match owners.as_slice() {
                [id] => ArtistId(id.clone()),
                [] => {
                    let id = ArtistId::new();
                    tx.execute(
                        "INSERT INTO artist(id,name) VALUES(?1,?2)",
                        params![id.as_ref(), proposal.candidate_name],
                    )?;
                    tx.execute(
                        "INSERT INTO artist_external_identity VALUES(?1,?2,?3,?4)",
                        params![
                            id.as_ref(),
                            proposal.candidate_identity.provider,
                            proposal.candidate_identity.kind,
                            proposal.candidate_identity.external_id
                        ],
                    )?;
                    id
                }
                _ => return Err(Error::Invalid(
                    "Provider Artist has multiple canonical owners; review those identities first"
                        .into(),
                )),
            };
        if other != proposal.local {
            let (a, b) = if proposal.local.0 < other.0 {
                (&proposal.local, &other)
            } else {
                (&other, &proposal.local)
            };
            tx.execute("INSERT INTO artist_equivalence(artist_a,artist_b) VALUES(?1,?2) ON CONFLICT DO NOTHING",params![a.as_ref(),b.as_ref()])?;
        }
        tx.commit()?;
        Ok(other)
    }
}
