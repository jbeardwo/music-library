//! Offline, canonical Library connection management. Provider matching stays separate.
use crate::{
    Error, Library, Result,
    domain::{AlbumId, TrackId},
};
use rusqlite::params;

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Provider {
    pub key: &'static str,
    pub name: &'static str,
}
pub const PROVIDERS: [Provider; 3] = [
    Provider {
        key: "local",
        name: "Local",
    },
    Provider {
        key: "musicbrainz",
        name: "MusicBrainz",
    },
    Provider {
        key: "spotify",
        name: "Spotify",
    },
];
fn connected(provider: &str) -> Result<&'static str> {
    match provider {
        "local" => Ok("connected_local_track"),
        "musicbrainz" => Ok("connected_musicbrainz_track"),
        "spotify" => Ok("trusted_spotify_track"),
        _ => Err(Error::Invalid("Unknown connection provider".into())),
    }
}
#[derive(Debug, serde::Serialize)]
pub struct Group {
    pub id: AlbumId,
    pub title: String,
    pub artist: String,
    pub matched: i64,
    pub total: i64,
    pub relevant: i64,
}
#[derive(Debug, serde::Serialize)]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    pub reason: String,
}
#[derive(Debug, serde::Serialize)]
pub struct Coverage {
    pub saved: i64,
    pub connected: i64,
    pub missing: i64,
    pub marked: i64,
    pub albums_missing: i64,
}
impl Library {
    /// Indexed Artist/Album keyset pagination; only the selected page is aggregated.
    pub fn connection_groups(
        &self,
        provider: &str,
        marked: bool,
        after: Option<&AlbumId>,
        limit: u32,
    ) -> Result<Vec<Group>> {
        let c = connected(provider)?;
        let relevant = format!(
            "{}EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider=?1){}",
            if marked { "" } else { "NOT " },
            if marked {
                String::new()
            } else {
                format!(" AND NOT EXISTS(SELECT 1 FROM {c} c WHERE c.track_id=t.id)")
            }
        );
        let source = if marked {
            "(SELECT DISTINCT r.album_id FROM track_provider_exclusion x INDEXED BY provider_exclusion_list JOIN library_membership m ON m.track_id=x.track_id JOIN track t ON t.id=x.track_id JOIN release r ON r.id=t.release_id WHERE x.provider=?1) z JOIN album_browse_order b ON b.album_id=z.album_id"
        } else {
            "album_browse_order b INDEXED BY connections_album_order"
        };
        let after_clause = if after.is_some() {
            "(b.artist_key,b.title_key,b.album_id)>(SELECT artist_key,title_key,album_id FROM album_browse_order WHERE album_id=?2)"
        } else {
            "?2 IS NULL"
        };
        let sql = format!(
            "WITH chosen AS MATERIALIZED (SELECT b.* FROM {source} WHERE {after_clause} AND EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id=b.album_id AND {relevant}) ORDER BY b.artist_key,b.title_key,b.album_id LIMIT ?3) SELECT b.album_id,a.title,COALESCE(a.artist_credit,a.match_artist_credit),sum(EXISTS(SELECT 1 FROM {c} c WHERE c.track_id=t.id)),count(*),sum({relevant}) FROM chosen b JOIN effective_album_metadata a ON a.album_id=b.album_id JOIN release r ON r.album_id=b.album_id JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id GROUP BY b.album_id ORDER BY b.artist_key,b.title_key,b.album_id"
        );
        Ok(self
            .store
            .connection
            .prepare(&sql)?
            .query_map(
                params![provider, after.map(AsRef::as_ref), limit.clamp(1, 200)],
                |r| {
                    Ok(Group {
                        id: AlbumId(r.get(0)?),
                        title: r.get(1)?,
                        artist: r.get(2)?,
                        matched: r.get(3)?,
                        total: r.get(4)?,
                        relevant: r.get(5)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn connection_tracks(
        &self,
        provider: &str,
        marked: bool,
        album: &AlbumId,
        after: Option<&TrackId>,
        limit: u32,
    ) -> Result<Vec<Track>> {
        let c = connected(provider)?;
        let reason = match provider {
            "local" => "'No usable local file'",
            "musicbrainz" => {
                "COALESCE(json_extract(ms.track_reasons,'$.\"'||t.id||'\".label'),CASE ms.reason WHEN 'version_uncertain' THEN 'Track version needs review' WHEN 'insufficient_evidence' THEN 'Candidate retained — insufficient correspondence evidence' WHEN 'identity_conflict' THEN 'Trusted identity conflicts' WHEN 'duration_conflict' THEN 'Track duration conflicts' WHEN 'matching' THEN 'Matching…' WHEN 'candidates_found' THEN 'Candidate found — needs review' WHEN 'album_title_conflict' THEN 'Candidate rejected: Album title conflict' WHEN 'artist_unresolved' THEN 'Artist identity needs review' WHEN 'no_candidates' THEN 'No candidates after bounded search' WHEN 'provider_deferred' THEN 'Provider temporarily unavailable' WHEN 'provider_error' THEN 'Provider request failed' WHEN 'program_incomplete' THEN 'Candidate program incomplete' WHEN 'recording_ambiguous' THEN 'Recording correspondence ambiguous' WHEN 'partial_mapping' THEN 'Candidate retained — partial mapping' WHEN 'evidence_changed' THEN 'Evidence changed — re-evaluate' WHEN 'program_retained' THEN 'Candidate program retained' ELSE 'Not checked yet' END)"
            }
            _ => {
                "COALESCE((SELECT reason FROM spotify_connection_review v WHERE v.track_id=t.id),'No reconciliation attempted yet')"
            }
        };
        let lifecycle_join = if provider == "musicbrainz" {
            "LEFT JOIN musicbrainz_match_state ms ON ms.album_id=r.album_id"
        } else {
            ""
        };
        let sql = format!(
            "SELECT t.id,e.title,{reason} FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id JOIN effective_track_metadata e ON e.track_id=t.id {lifecycle_join} WHERE r.album_id=?1 AND (?4 IS NULL OR (COALESCE(t.disc_number,1),COALESCE(t.track_number,0),t.id)>(SELECT COALESCE(disc_number,1),COALESCE(track_number,0),id FROM track WHERE id=?4)) AND {}EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider=?2) {} ORDER BY COALESCE(t.disc_number,1),COALESCE(t.track_number,0),t.id LIMIT ?3",
            if marked { "" } else { "NOT " },
            if marked {
                String::new()
            } else {
                format!("AND NOT EXISTS(SELECT 1 FROM {c} c WHERE c.track_id=t.id)")
            }
        );
        Ok(self
            .store
            .connection
            .prepare(&sql)?
            .query_map(
                params![
                    album.as_ref(),
                    provider,
                    limit.clamp(1, 200),
                    after.map(AsRef::as_ref)
                ],
                |r| {
                    Ok(Track {
                        id: TrackId(r.get(0)?),
                        title: r.get(1)?,
                        reason: r.get(2)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn connection_coverage(&self, provider: &str) -> Result<Coverage> {
        let c = connected(provider)?;
        let sql = format!(
            "SELECT count(*),COALESCE(sum(EXISTS(SELECT 1 FROM {c} c WHERE c.track_id=t.id)),0),COALESCE(sum(EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider=?1)),0),count(DISTINCT CASE WHEN NOT EXISTS(SELECT 1 FROM {c} c WHERE c.track_id=t.id) THEN r.album_id END) FROM library_membership m JOIN track t ON t.id=m.track_id JOIN release r ON r.id=t.release_id"
        );
        Ok(self.store.connection.query_row(&sql, [provider], |r| {
            let saved = r.get(0)?;
            let connected = r.get(1)?;
            Ok(Coverage {
                saved,
                connected,
                missing: saved - connected,
                marked: r.get(2)?,
                albums_missing: r.get(3)?,
            })
        })?)
    }
    /// Explicit catalog Recording confirmation; conflict check and write are atomic.
    /// This does not infer canonical Release equivalence or change membership.
    pub fn confirm_musicbrainz_connection(
        &mut self,
        track: &TrackId,
        identity: &crate::domain::ExternalIdentity,
    ) -> Result<()> {
        self.confirm_musicbrainz_connections(&[(track.clone(), identity.clone())])?;
        Ok(())
    }
    /// One explicit Album action; validation and persistence share a short transaction.
    pub fn confirm_musicbrainz_connections(
        &mut self,
        mappings: &[(TrackId, crate::domain::ExternalIdentity)],
    ) -> Result<usize> {
        let tx = self
            .store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for (track, identity) in mappings {
            if identity.provider != "musicbrainz"
                || identity.kind != "recording"
                || identity.external_id.is_empty()
            {
                return Err(Error::Invalid(
                    "Choose a MusicBrainz Recording identity".into(),
                ));
            }
            let recording: String = tx.query_row(
                "SELECT recording_id FROM track WHERE id=?1",
                [track.as_ref()],
                |r| r.get(0),
            )?;
            let conflict:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM recording_external_identity WHERE recording_id=?1 AND provider='musicbrainz' AND kind='recording' AND external_id<>?2)",params![recording,identity.external_id],|r|r.get(0))?;
            if conflict {
                return Err(Error::Invalid(
                    "Conflicting trusted Recording identity; use the correction workflow".into(),
                ));
            }
            tx.execute("INSERT INTO recording_external_identity(recording_id,provider,kind,external_id) VALUES(?1,'musicbrainz','recording',?2) ON CONFLICT DO NOTHING",params![recording,identity.external_id])?;
            tx.execute(
                "DELETE FROM track_provider_exclusion WHERE track_id=?1 AND provider='musicbrainz'",
                [track.as_ref()],
            )?;
        }
        tx.commit()?;
        Ok(mappings.len())
    }
    /// Existing reconciliation program retained for offline human review. This is
    /// evidence to re-evaluate, never permission to establish an association.
    pub fn retained_spotify_album_program(
        &self,
        album: &AlbumId,
    ) -> Result<Option<crate::album_program::Programs>> {
        use rusqlite::OptionalExtension;
        let value: Option<String> = self
            .store
            .connection
            .query_row(
                "SELECT programs_json FROM spotify_program_cache WHERE album_id=?1",
                [album.as_ref()],
                |r| r.get(0),
            )
            .optional()?;
        value
            .map(|v| serde_json::from_str(&v).map_err(|e| Error::Invalid(e.to_string())))
            .transpose()
    }
    /// Presentation of effective canonical evidence, independent of any source.
    pub fn connection_review_known(
        &self,
        album: &AlbumId,
        track: Option<&TrackId>,
    ) -> Result<serde_json::Value> {
        let (title,artist,year,kind):(String,String,Option<i32>,Option<String>)=self.store.connection.query_row("SELECT a.title,COALESCE(a.artist_credit,a.match_artist_credit),COALESCE(a.year,b.year),a.release_type FROM effective_album_metadata a JOIN album_browse_order b ON b.album_id=a.album_id WHERE a.album_id=?1",[album.as_ref()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        if let Some(track) = track {
            if self.album_id_for_track(track)? != *album {
                return Err(Error::Invalid("Track is outside the reviewed Album".into()));
            }
            let input = self.song_resolution_input(track)?;
            return Ok(
                serde_json::json!({"title":input.title,"artist":input.artist,"album":input.album,"date":year.map(|y|y.to_string()).unwrap_or_default(),"kind":kind,"length":input.duration_ms.map(|d|format!("{}:{:02}",d/60000,d/1000%60)).unwrap_or_default(),"position":input.number.map(|n|format!("{}.{}",input.disc.unwrap_or(1),n)).unwrap_or_default(),"program":[]}),
            );
        }
        let mut q=self.store.connection.prepare("SELECT e.title,COALESCE(t.disc_number,1),t.track_number FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=?1 ORDER BY COALESCE(t.disc_number,1),COALESCE(t.track_number,0),t.id LIMIT 200")?;
        let program = q
            .query_map([album.as_ref()], |r| {
                Ok(format!(
                    "{}.{} {}",
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "?".into()),
                    r.get::<_, String>(0)?
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let evidence = crate::canonical_evidence::load_album(&self.store.connection, album)?;
        Ok(
            serde_json::json!({"title":title,"artist":artist,"date":year.map(|y|y.to_string()).or_else(||evidence.dates.first().map(|d|d.value.clone())).unwrap_or_default(),"kind":kind.or_else(||evidence.release_types.first().map(|t|t.value.clone())),"program":program}),
        )
    }

    /// Snapshot current unresolved saved Tracks. No Album rule is stored.
    pub fn mark_connection_unavailable(
        &mut self,
        provider: &str,
        album: Option<&AlbumId>,
        track: Option<&TrackId>,
    ) -> Result<usize> {
        let c = connected(provider)?;
        if album.is_none() && track.is_none() {
            return Err(Error::Invalid("Select an Album or Track".into()));
        }
        Ok(self.store.connection.execute(&format!("INSERT INTO track_provider_exclusion(track_id,provider) SELECT t.id,?1 FROM track t JOIN release r ON r.id=t.release_id JOIN library_membership m ON m.track_id=t.id WHERE (?2 IS NULL OR r.album_id=?2) AND (?3 IS NULL OR t.id=?3) AND NOT EXISTS(SELECT 1 FROM {c} c WHERE c.track_id=t.id) ON CONFLICT DO NOTHING"),params![provider,album.map(AsRef::as_ref),track.map(AsRef::as_ref)])?)
    }
    pub fn clear_connection_unavailable(&mut self, provider: &str, track: &TrackId) -> Result<()> {
        connected(provider)?;
        if provider == "spotify" {
            return self.check_spotify_again(track);
        }
        self.store.connection.execute(
            "DELETE FROM track_provider_exclusion WHERE provider=?1 AND track_id=?2",
            params![provider, track.as_ref()],
        )?;
        Ok(())
    }
}

impl Library {
    /// Album-scoped retained review evidence; not a trusted association.
    pub fn retain_connection_candidate(
        &mut self,
        album: &AlbumId,
        market: &str,
        candidate: &crate::catalog::ArtistAlbumCandidate,
        programs: &crate::album_program::Programs,
    ) -> Result<()> {
        let mut rows = self.retained_connection_candidates(album, market)?;
        rows.retain(|(c, _)| c.identity != candidate.identity);
        rows.insert(0, (candidate.clone(), programs.clone()));
        rows.truncate(10);
        self.persist_spotify_discovery(
            &format!("spotify-review:1:{market}:{}", album.as_ref()),
            &serde_json::to_string(&rows).map_err(|e| Error::Invalid(e.to_string()))?,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        )
    }
    pub fn retained_connection_candidates(
        &self,
        album: &AlbumId,
        market: &str,
    ) -> Result<
        Vec<(
            crate::catalog::ArtistAlbumCandidate,
            crate::album_program::Programs,
        )>,
    > {
        self.spotify_discovery_observation(&format!(
            "spotify-review:1:{market}:{}",
            album.as_ref()
        ))?
        .map(|s| serde_json::from_str(&s).map_err(|e| Error::Invalid(e.to_string())))
        .transpose()
        .map(|v| v.unwrap_or_default())
    }
}
