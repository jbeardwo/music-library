//! Explicit MusicBrainz v2 requests. JSON types never leave this adapter.
use music_library::{
    catalog::{self, CatalogError, CatalogProvider},
    domain::ExternalIdentity,
};
use serde::Deserialize;
use std::{
    sync::Mutex,
    thread,
    time::{Duration, Instant, SystemTime},
};
use url::Url;

pub const USER_AGENT: &str = concat!(
    "music-library/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/jbeardwo/music-library)"
);
// Ten initial discovery results; explicit next-page requests retain access to the rest.
const PAGE_SIZE: u32 = 10;
const MAX_BODY: u64 = 8 * 1024 * 1024;
// Shared by all clients in this process, not independently by UI callers.
static NEXT_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);

pub struct MusicBrainz {
    agent: ureq::Agent,
    base: Url,
}
impl Default for MusicBrainz {
    fn default() -> Self {
        Self::new()
    }
}
impl MusicBrainz {
    pub fn new() -> Self {
        Self::at(Url::parse("https://musicbrainz.org/ws/2/").unwrap())
    }
    fn at(base: Url) -> Self {
        Self {
            base,
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(30)))
                .max_redirects(0)
                .http_status_as_error(false)
                .user_agent(USER_AGENT)
                .build()
                .into(),
        }
    }
    fn request<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T, CatalogError> {
        let mut url = self.base.join(path).map_err(error)?;
        url.query_pairs_mut()
            .append_pair("fmt", "json")
            .extend_pairs(params.iter().map(|(k, v)| (*k, v.as_str())));
        // Keep every attempt behind the same process-wide gate. Holding it through
        // backoff also prevents another client from bypassing this service cooldown.
        let mut wait = catalog::Timing::new(format!("{path}.rate_wait"));
        let mut next = NEXT_REQUEST.lock().map_err(error)?;
        for attempt in 0..=2 {
            if let Some(deadline) = *next {
                thread::sleep(deadline.saturating_duration_since(Instant::now()));
            }
            drop(wait);
            *next = Some(Instant::now() + Duration::from_secs(1));
            catalog::Timing::event(format_args!(
                "http_begin path={path} attempt={}",
                attempt + 1
            ));
            let http = catalog::Timing::new(format!("{path}.http"));
            let headers = catalog::Timing::new(format!("{path}.http_headers"));
            let mut response = self
                .agent
                .get(url.as_str())
                .header("Accept", "application/json")
                .call()
                .map_err(|e| {
                    catalog::Timing::event(format_args!(
                        "http_error path={path} attempt={} error={e}",
                        attempt + 1
                    ));
                    transport_error(e)
                })?;
            drop(headers);
            let status = response.status().as_u16();
            catalog::Timing::event(format_args!(
                "http_status path={path} attempt={} status={status}",
                attempt + 1
            ));
            if status == 503
                && let Some(delay) = retry_delay(
                    attempt,
                    response
                        .headers()
                        .get("Retry-After")
                        .and_then(|v| v.to_str().ok()),
                    SystemTime::now(),
                )
            {
                catalog::Timing::event(format_args!(
                    "retry path={path} backoff_ms={}",
                    delay.as_millis()
                ));
                let deadline = Instant::now() + delay;
                *next = Some(next.map_or(deadline, |previous| previous.max(deadline)));
                // Drop the failed response before waiting; no intermediate error reaches Qt.
                drop(response);
                drop(http);
                wait = catalog::Timing::new(format!("{path}.rate_wait"));
                continue;
            }
            if !response.status().is_success() {
                let message = format!("MusicBrainz request failed: HTTP {status}");
                return Err(if status == 503 {
                    CatalogError::ServiceUnavailable {
                        message,
                        retry_after: response
                            .headers()
                            .get("Retry-After")
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_owned),
                    }
                } else {
                    CatalogError::Other(message)
                });
            }
            let body_read = catalog::Timing::new(format!("{path}.http_body"));
            let body = response
                .body_mut()
                .with_config()
                .limit(MAX_BODY)
                .read_to_string()
                .map_err(transport_error)?;
            drop(body_read);
            catalog::Timing::event(format_args!("http_body path={path} bytes={}", body.len()));
            drop(http);
            drop(next);
            let _parse = catalog::Timing::new(format!("{path}.json_parse"));
            return serde_json::from_str(&body)
                .map_err(|e| CatalogError::Other(format!("Invalid MusicBrainz response: {e}")));
        }
        unreachable!("the final attempt returns its response or error")
    }
}
impl MusicBrainz {
    fn browse_releases(
        &self,
        group: &ExternalIdentity,
        offset: u32,
        includes: &str,
        official: bool,
    ) -> Result<catalog::Page<catalog::ReleaseCandidate>, CatalogError> {
        require_identity(group, "release_group")?;
        let mut params = vec![
            ("release-group", group.external_id.clone()),
            ("inc", includes.into()),
            ("limit", "100".into()),
            ("offset", offset.to_string()),
        ];
        if official {
            params.push(("status", "official".into()));
        }
        let page: Releases = self.request("release", &params)?;
        let _conversion = catalog::Timing::new("edition_browse.conversion");
        catalog::Timing::event(format_args!(
            "editions_returned={} total={} offset={offset}",
            page.releases.len(),
            page.count
        ));
        let next_offset = next_offset(offset, page.releases.len(), page.count);
        let items = page
            .releases
            .into_iter()
            .map(|r| {
                mbid(&r.id)?;
                Ok(catalog::ReleaseCandidate {
                    identity: identity("release", &r.id),
                    title: r.title,
                    artist: catalog::credit_display(&credits(r.credits)),
                    date: r.date.unwrap_or_default(),
                    country: r.country.unwrap_or_default(),
                    status: r.status.unwrap_or_default(),
                    comment: r.disambiguation.unwrap_or_default(),
                    barcode: r.barcode.unwrap_or_default(),
                    labels: r
                        .labels
                        .into_iter()
                        .map(|l| {
                            format!(
                                "{} {}",
                                l.label.map(|l| l.name).unwrap_or_default(),
                                l.catalog_number.unwrap_or_default()
                            )
                            .trim()
                            .into()
                        })
                        .collect(),
                    disc_count: r.media.len(),
                    track_count: r.media.iter().map(|m| m.track_count).sum(),
                    formats: r
                        .media
                        .into_iter()
                        .map(|m| m.format.unwrap_or_default())
                        .collect(),
                })
            })
            .collect::<Result<_, CatalogError>>()?;
        Ok(catalog::Page { items, next_offset })
    }
}
// Retry-After accepts delay-seconds or HTTP-date (RFC 9110 §10.2.3).
// Decline automatic retry rather than shorten a server delay longer than one minute.
fn retry_delay(attempt: u32, header: Option<&str>, now: SystemTime) -> Option<Duration> {
    if attempt >= 2 {
        return None;
    }
    let delay = header
        .and_then(|value| {
            let value = value.trim();
            if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
                // Overflowing delay-seconds means too long, not permission to retry sooner.
                Some(Duration::from_secs(value.parse().unwrap_or(u64::MAX)))
            } else {
                httpdate::parse_http_date(value)
                    .ok()
                    .map(|date| date.duration_since(now).unwrap_or_default())
            }
        })
        .unwrap_or_else(|| Duration::from_secs(2 << attempt));
    (delay <= Duration::from_secs(60)).then_some(delay)
}

fn transport_error(error: ureq::Error) -> CatalogError {
    let message = format!("MusicBrainz request failed: {error}");
    match error {
        ureq::Error::Timeout(_) => CatalogError::Timeout(message),
        ureq::Error::Io(_) | ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => {
            CatalogError::TransportUnavailable(message)
        }
        _ => CatalogError::Other(message),
    }
}

fn error(e: impl std::fmt::Display) -> CatalogError {
    CatalogError::Other(e.to_string())
}
fn identity(kind: &str, id: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "musicbrainz".into(),
        kind: kind.into(),
        external_id: id.into(),
    }
}
fn mbid(id: &str) -> Result<(), CatalogError> {
    if id.len() != 36
        || !id.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        return Err(CatalogError::Other("Invalid MusicBrainz identifier".into()));
    }
    Ok(())
}
fn require_identity(id: &ExternalIdentity, kind: &str) -> Result<(), CatalogError> {
    if id.provider != "musicbrainz" || id.kind != kind {
        return Err(CatalogError::Other(
            "Unsupported MusicBrainz identity kind/provider".into(),
        ));
    }
    mbid(&id.external_id)
}
fn next_offset(offset: u32, len: usize, count: u32) -> Option<u32> {
    let next = offset.saturating_add(len as u32);
    (len > 0 && next < count).then_some(next)
}
impl MusicBrainz {
    fn scoped_artist_albums(
        &mut self,
        artists: &[ExternalIdentity],
        title: &str,
        max_edits: u8,
    ) -> Result<catalog::Page<catalog::ArtistAlbumCandidate>, CatalogError> {
        if artists.is_empty() || artists.len() > PAGE_SIZE as usize {
            return Err(CatalogError::Other(
                "Expected a bounded nonempty Artist candidate set".into(),
            ));
        }
        let mut scopes = Vec::new();
        for artist in artists {
            require_identity(artist, "artist")?;
            scopes.push(format!("arid:{}", artist.external_id));
        }
        scopes.sort();
        scopes.dedup();
        let scope = if scopes.len() == 1 {
            scopes.remove(0)
        } else {
            format!("({})", scopes.join(" OR "))
        };
        // Candidate retrieval permits bounded edit tokens only inside this Artist.
        // Full-title Unicode edit distance and ambiguity checks belong to the application.
        let fuzzy = title
            .split_whitespace()
            .map(|word| format!("releasegroup:{}~{max_edits}", escaped(word)))
            .collect::<Vec<_>>()
            .join(" AND ");
        let terms = if title.chars().count() >= 5 {
            format!("(releasegroup:{} OR ({fuzzy}))", quoted(title))
        } else {
            format!("releasegroup:{}", quoted(title))
        };
        let mut alternatives = vec![terms];
        alternatives.extend(
            music_library::album_matching::album_title_variants(title)
                .into_iter()
                .map(|v| format!("releasegroup:{}", quoted(&v.title))),
        );
        let terms = if alternatives.len() == 1 {
            alternatives.remove(0)
        } else {
            format!("({})", alternatives.join(" OR "))
        };
        let query = format!("{scope} AND {terms}");
        let page: Groups = self.request(
            "release-group",
            &[
                ("query", query),
                ("limit", PAGE_SIZE.to_string()),
                ("offset", "0".into()),
            ],
        )?;
        let next_offset = next_offset(0, page.groups.len(), page.count);
        let items = page
            .groups
            .into_iter()
            .map(|g| {
                mbid(&g.id)?;
                let ids = g
                    .credits
                    .iter()
                    .map(|c| {
                        let id = &c
                            .artist
                            .as_ref()
                            .ok_or_else(|| {
                                CatalogError::Other("Release Group lacks Artist identity".into())
                            })?
                            .id;
                        mbid(id)?;
                        Ok(identity("artist", id))
                    })
                    .collect::<Result<Vec<_>, CatalogError>>()?;
                Ok(catalog::ArtistAlbumCandidate {
                    artist: g
                        .credits
                        .iter()
                        .map(|c| {
                            let name = c
                                .artist
                                .as_ref()
                                .and_then(|a| a.name.as_deref())
                                .unwrap_or(&c.name);
                            format!("{name}{}", c.joinphrase)
                        })
                        .collect(),
                    primary_type: g.primary_type.unwrap_or_default(),
                    identity: identity("release_group", &g.id),
                    title: g.title,
                    artist_ids: ids,
                    comment: g.disambiguation.unwrap_or_default(),
                    date: g.date.unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>, CatalogError>>()?;
        Ok(catalog::Page { items, next_offset })
    }
}
impl CatalogProvider for MusicBrainz {
    fn recordings(
        &mut self,
        group: &ExternalIdentity,
    ) -> Result<catalog::Page<music_library::recording::Candidate>, CatalogError> {
        require_identity(group, "release_group")?;
        let page: RecordingSearch = self.request(
            "recording",
            &[
                ("query", format!("rgid:{}", group.external_id)),
                ("limit", "100".into()),
                ("offset", "0".into()),
            ],
        )?;
        // Do not mistake a malformed short/empty page with a positive count for completeness.
        let next_offset =
            (page.count as usize > page.recordings.len()).then_some(page.recordings.len() as u32);
        let items = page
            .recordings
            .into_iter()
            .map(|r| {
                mbid(&r.id)?;
                let artist_ids = r
                    .credits
                    .iter()
                    .map(|c| {
                        let a = c.artist.as_ref().ok_or_else(|| {
                            CatalogError::Other("Recording credit lacks Artist identity".into())
                        })?;
                        mbid(&a.id)?;
                        Ok(identity("artist", &a.id))
                    })
                    .collect::<Result<Vec<_>, CatalogError>>()?;
                Ok(music_library::recording::Candidate {
                    identity: identity("recording", &r.id),
                    title: r.title,
                    artist: r
                        .credits
                        .iter()
                        .map(|c| format!("{}{}", c.name, c.joinphrase))
                        .collect(),
                    artist_ids,
                    duration_ms: r.length,
                    isrcs: r.isrcs,
                })
            })
            .collect::<Result<Vec<_>, CatalogError>>()?;
        Ok(catalog::Page { items, next_offset })
    }
    fn search_artists(
        &mut self,
        name: &str,
    ) -> Result<catalog::Page<catalog::ArtistCandidate>, CatalogError> {
        let page: Artists = self.request(
            "artist",
            &[
                (
                    "query",
                    format!("(artist:{} OR alias:{})", quoted(name), quoted(name)),
                ),
                ("limit", PAGE_SIZE.to_string()),
                ("offset", "0".into()),
            ],
        )?;
        let next_offset = next_offset(0, page.artists.len(), page.count);
        let items = page
            .artists
            .into_iter()
            .map(|a| {
                mbid(&a.id)?;
                Ok(catalog::ArtistCandidate {
                    aliases: a.aliases.into_iter().map(|v| v.name).collect(),
                    identity: identity("artist", &a.id),
                    name: a.name,
                    comment: a.disambiguation.unwrap_or_default(),
                    country: a.country.unwrap_or_default(),
                    artist_type: a.artist_type.unwrap_or_default(),
                    score: a.score,
                })
            })
            .collect::<Result<Vec<_>, CatalogError>>()?;
        Ok(catalog::Page { items, next_offset })
    }
    fn artist_albums(
        &mut self,
        artist: &ExternalIdentity,
        title: &str,
    ) -> Result<catalog::Page<catalog::ArtistAlbumCandidate>, CatalogError> {
        self.scoped_artist_albums(std::slice::from_ref(artist), title, 1)
    }
    fn albums_for_artists(
        &mut self,
        artists: &[ExternalIdentity],
        title: &str,
    ) -> Result<catalog::Page<catalog::ArtistAlbumCandidate>, CatalogError> {
        self.scoped_artist_albums(artists, title, 1)
    }
    fn artist_albums_confirmed(
        &mut self,
        artist: &ExternalIdentity,
        title: &str,
    ) -> Result<catalog::Page<catalog::ArtistAlbumCandidate>, CatalogError> {
        self.scoped_artist_albums(std::slice::from_ref(artist), title, 2)
    }
    fn search_albums(
        &mut self,
        query: &str,
        offset: u32,
    ) -> Result<catalog::Page<catalog::AlbumCandidate>, CatalogError> {
        if query.trim().is_empty() {
            return Err(CatalogError::Other("Enter a catalog search query".into()));
        }
        let page: Groups = self.request(
            "release-group",
            &[
                ("query", query.into()),
                ("limit", PAGE_SIZE.to_string()),
                ("offset", offset.to_string()),
            ],
        )?;
        let _conversion = catalog::Timing::new("album_search.conversion");
        let next_offset = next_offset(offset, page.groups.len(), page.count);
        let items = page
            .groups
            .into_iter()
            .map(|g| {
                mbid(&g.id)?;
                Ok(catalog::AlbumCandidate {
                    identity: identity("release_group", &g.id),
                    title: g.title,
                    artist: catalog::credit_display(&credits(g.credits.clone())),
                    credits: credits(g.credits),
                    date: g.date.unwrap_or_default(),
                    primary_type: g.primary_type.unwrap_or_default(),
                    secondary_types: g.secondary_types,
                    comment: g.disambiguation.unwrap_or_default(),
                    score: g.score,
                })
            })
            .collect::<Result<_, CatalogError>>()?;
        Ok(catalog::Page { items, next_offset })
    }
    fn releases(
        &mut self,
        group: &ExternalIdentity,
        offset: u32,
    ) -> Result<catalog::Page<catalog::ReleaseCandidate>, CatalogError> {
        self.browse_releases(group, offset, "artist-credits+labels+media", false)
    }
    fn representative_releases(
        &mut self,
        group: &ExternalIdentity,
    ) -> Result<catalog::Page<catalog::ReleaseCandidate>, CatalogError> {
        // Media summaries retain the empty-track/disc checks without edition-display
        // credits/labels. Browse supports status filtering and paging (not the
        // 25-linked-entity limit of Release Group lookup):
        // https://musicbrainz.org/doc/MusicBrainz_API#Browse
        let page = self.browse_releases(group, 0, "media", true)?;
        if page.items.is_empty() {
            catalog::Timing::event("representative_fallback=no_official_releases");
            self.browse_releases(group, 0, "media", false)
        } else {
            Ok(page)
        }
    }
    fn release(&mut self, id: &ExternalIdentity) -> Result<catalog::Release, CatalogError> {
        require_identity(id, "release")?;
        let release: FullRelease = self.request(
            &format!("release/{}", id.external_id),
            &[(
                "inc",
                "release-groups+recordings+artist-credits+isrcs+media".into(),
            )],
        )?;
        if release.id != id.external_id {
            return Err(CatalogError::Other(
                "MusicBrainz returned a different Release".into(),
            ));
        }
        let _conversion = catalog::Timing::new("release_lookup.conversion");
        convert_release(release)
    }
}

fn escaped(value: &str) -> String {
    let mut result = String::new();
    for c in value.chars() {
        if "+-!():^[]\"{}~*?|&/\\".contains(c) {
            result.push('\\');
        }
        result.push(c);
    }
    result
}
fn quoted(value: &str) -> String {
    format!("\"{}\"", escaped(value))
}
#[derive(Deserialize)]
struct Artists {
    count: u32,
    artists: Vec<ArtistSearch>,
}
#[derive(Deserialize)]
struct ArtistSearch {
    #[serde(default)]
    aliases: Vec<ArtistAlias>,
    id: String,
    name: String,
    disambiguation: Option<String>,
    country: Option<String>,
    #[serde(rename = "type")]
    artist_type: Option<String>,
    score: Option<u32>,
}
#[derive(Deserialize)]
struct ArtistAlias {
    name: String,
}
#[derive(Clone, Deserialize)]
struct ArtistRef {
    id: String,
    name: Option<String>,
}

#[derive(Deserialize)]
struct Groups {
    count: u32,
    #[serde(rename = "release-groups")]
    groups: Vec<Group>,
}
#[derive(Deserialize)]
struct Group {
    id: String,
    title: String,
    #[serde(default, rename = "artist-credit")]
    credits: Vec<Credit>,
    #[serde(rename = "first-release-date")]
    date: Option<String>,
    #[serde(rename = "primary-type")]
    primary_type: Option<String>,
    #[serde(default, rename = "secondary-types")]
    secondary_types: Vec<String>,
    disambiguation: Option<String>,
    score: Option<u32>,
}
#[derive(Clone, Deserialize)]
struct Credit {
    name: String,
    artist: Option<ArtistRef>,
    #[serde(default)]
    joinphrase: String,
}
fn credits(values: Vec<Credit>) -> Vec<catalog::Credit> {
    values
        .into_iter()
        .map(|c| catalog::Credit {
            identity: c.artist.map(|a| identity("artist", &a.id)),
            name: c.name,
            join_phrase: c.joinphrase,
        })
        .collect()
}
#[derive(Deserialize)]
struct Releases {
    #[serde(rename = "release-count")]
    count: u32,
    releases: Vec<Edition>,
}
#[derive(Deserialize)]
struct Edition {
    id: String,
    title: String,
    #[serde(default, rename = "artist-credit")]
    credits: Vec<Credit>,
    date: Option<String>,
    country: Option<String>,
    status: Option<String>,
    disambiguation: Option<String>,
    barcode: Option<String>,
    #[serde(default, rename = "label-info")]
    labels: Vec<LabelInfo>,
    #[serde(default)]
    media: Vec<MediaSummary>,
}
#[derive(Deserialize)]
struct LabelInfo {
    label: Option<Label>,
    #[serde(rename = "catalog-number")]
    catalog_number: Option<String>,
}
#[derive(Deserialize)]
struct Label {
    name: String,
}
#[derive(Deserialize)]
struct MediaSummary {
    format: Option<String>,
    #[serde(rename = "track-count")]
    track_count: u32,
}
#[derive(Deserialize)]
struct FullRelease {
    id: String,
    title: String,
    date: Option<String>,
    #[serde(rename = "artist-credit")]
    credits: Vec<Credit>,
    #[serde(rename = "release-group")]
    group: GroupRef,
    media: Vec<Medium>,
}
#[derive(Deserialize)]
struct GroupRef {
    id: String,
    title: String,
    #[serde(rename = "first-release-date")]
    date: Option<String>,
    #[serde(default, rename = "artist-credit")]
    credits: Vec<Credit>,
}
#[derive(Deserialize)]
struct Medium {
    position: u32,
    #[serde(rename = "track-count")]
    track_count: usize,
    #[serde(default)]
    tracks: Vec<Track>,
    #[serde(default, rename = "data-tracks")]
    data_tracks: Vec<Track>,
    pregap: Option<Track>,
}
#[derive(Deserialize)]
struct Track {
    id: String,
    position: u32,
    title: Option<String>,
    #[serde(rename = "artist-credit")]
    credits: Option<Vec<Credit>>,
    recording: Recording,
}
#[derive(Deserialize)]
struct Recording {
    id: String,
    title: String,
    #[serde(default, rename = "artist-credit")]
    credits: Vec<Credit>,
    #[serde(default)]
    isrcs: Vec<String>,
    length: Option<u64>,
}
#[derive(Deserialize)]
struct RecordingSearch {
    count: u32,
    recordings: Vec<Recording>,
}
fn convert_release(mut r: FullRelease) -> Result<catalog::Release, CatalogError> {
    mbid(&r.id)?;
    mbid(&r.group.id)?;
    r.media.sort_by_key(|m| m.position);
    if r.media.is_empty() || r.media.windows(2).any(|m| m[0].position == m[1].position) {
        return Err(CatalogError::Other(
            "Release has missing or duplicate media".into(),
        ));
    }
    let release_credits = credits(r.credits);
    let media = r
        .media
        .into_iter()
        .map(|mut m| {
            if m.position == 0 || m.tracks.len() != m.track_count {
                return Err(CatalogError::Other(
                    "Release tracklist is incomplete".into(),
                ));
            }
            m.tracks.extend(m.data_tracks);
            m.tracks.extend(m.pregap);
            m.tracks.sort_by_key(|t| t.position);
            if m.tracks.windows(2).any(|t| t[0].position == t[1].position) {
                return Err(CatalogError::Other("Duplicate track positions".into()));
            }
            let tracks = m
                .tracks
                .into_iter()
                .map(|t| {
                    mbid(&t.id)?;
                    mbid(&t.recording.id)?;
                    let mut identities = vec![
                        identity("track", &t.id),
                        identity("recording", &t.recording.id),
                    ];
                    identities.extend(t.recording.isrcs.into_iter().map(|id| ExternalIdentity {
                        provider: "isrc".into(),
                        kind: "recording".into(),
                        external_id: id,
                    }));
                    let values = t.credits.unwrap_or(t.recording.credits);
                    let artist = if values.is_empty() {
                        release_credits.clone()
                    } else {
                        credits(values)
                    };
                    Ok(catalog::Track {
                        position: t.position,
                        title: t.title.unwrap_or(t.recording.title),
                        credits: artist,
                        identities,
                    })
                })
                .collect::<Result<Vec<_>, CatalogError>>()?;
            Ok(catalog::Medium {
                position: m.position,
                tracks,
            })
        })
        .collect::<Result<Vec<_>, CatalogError>>()?;
    if media.iter().all(|m| m.tracks.is_empty()) {
        return Err(CatalogError::Other("Release has no Tracks".into()));
    }
    Ok(catalog::Release {
        album: catalog::Album {
            identity: identity("release_group", &r.group.id),
            title: r.group.title,
            date: r.group.date.unwrap_or_default(),
            credits: if r.group.credits.is_empty() {
                release_credits.clone()
            } else {
                credits(r.group.credits)
            },
        },
        identity: identity("release", &r.id),
        identities: vec![],
        title: r.title,
        date: r.date.unwrap_or_default(),
        credits: release_credits,
        media,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn transport_errors_are_typed_without_string_matching() {
        assert!(matches!(
            super::transport_error(ureq::Error::Timeout(ureq::Timeout::Global)),
            CatalogError::Timeout(_)
        ));
        assert!(super::transport_error(ureq::Error::HostNotFound).is_provider_unavailable());
        assert!(
            super::transport_error(ureq::Error::Io(std::io::Error::from(
                std::io::ErrorKind::ConnectionRefused
            )))
            .is_provider_unavailable()
        );
        assert!(
            !super::transport_error(ureq::Error::BadUri("bad".into())).is_provider_unavailable()
        );
    }
    #[test]
    fn credits_retain_artist_identity_separately_from_printed_name() {
        let values: Vec<super::Credit> = serde_json::from_str(
            r#"[
          {"name":"Printed alias","joinphrase":" feat. ","artist":{"id":"artist-mbid"}},
          {"name":"Text only"}
        ]"#,
        )
        .unwrap();
        let converted = super::credits(values);
        assert_eq!(
            converted[0].identity,
            Some(super::identity("artist", "artist-mbid"))
        );
        assert_eq!(converted[0].name, "Printed alias");
        assert_eq!(converted[0].join_phrase, " feat. ");
        assert_eq!(converted[1].identity, None);
    }

    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
    };
    const GROUPS: &str = include_str!("../tests/fixtures/groups.json");
    const EDITIONS: &str = include_str!("../tests/fixtures/editions.json");
    const RELEASE: &str = include_str!("../tests/fixtures/release.json");
    fn mock(
        responses: Vec<(u16, &str)>,
    ) -> (
        MusicBrainz,
        mpsc::Receiver<(Instant, String)>,
        thread::JoinHandle<()>,
    ) {
        mock_with_headers(
            responses
                .into_iter()
                .map(|(status, body)| (status, body, ""))
                .collect(),
        )
    }
    fn mock_with_headers(
        responses: Vec<(u16, &str, &str)>,
    ) -> (
        MusicBrainz,
        mpsc::Receiver<(Instant, String)>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = Url::parse(&format!("http://{}/ws/2/", listener.local_addr().unwrap())).unwrap();
        let (send, recv) = mpsc::channel();
        let responses: Vec<_> = responses
            .into_iter()
            .map(|(status, body, headers)| (status, body.to_owned(), headers.to_owned()))
            .collect();
        let worker = thread::spawn(move || {
            for (status, body, headers) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                send.send((Instant::now(), String::from_utf8(request).unwrap()))
                    .unwrap();
                write!(
                    stream,
                    "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        (MusicBrainz::at(base), recv, worker)
    }
    #[test]
    fn ambiguous_artists_use_one_combined_artist_scoped_album_request() {
        let artists = r#"{"count":2,"artists":[{"id":"00000000-0000-4000-8000-000000000001","name":"Kid Floral","aliases":[{"name":"Floral"}]},{"id":"00000000-0000-4000-8000-000000000002","name":"Floral"}]}"#;
        let groups = r#"{"count":1,"release-groups":[{"id":"00000000-0000-4000-8000-000000000003","title":"Floral LP","primary-type":"Album","artist-credit":[{"name":"Floral","artist":{"id":"00000000-0000-4000-8000-000000000002"}}]}]}"#;
        let (mut client, requests, server) = mock(vec![(200, artists), (200, groups)]);
        let artist_page = client.search_artists("Floral").unwrap();
        let Err(music_library::album_matching::MatchOutcome::ArtistAmbiguous(candidates)) =
            music_library::album_matching::resolve_artist("Floral", &artist_page)
        else {
            panic!("expected ambiguous Artist evidence");
        };
        let ids = candidates
            .iter()
            .map(|c| c.identity.clone())
            .collect::<Vec<_>>();
        let page = client.albums_for_artists(&ids, "Floral LP").unwrap();
        let (chosen, outcome) = music_library::album_matching::corroborate_artist_album(
            "Floral",
            "Floral LP",
            &candidates,
            &page,
        );
        assert_eq!(
            chosen,
            Some(identity("artist", "00000000-0000-4000-8000-000000000002"))
        );
        assert!(matches!(
            outcome,
            music_library::album_matching::MatchOutcome::Matched(_)
        ));
        server.join().unwrap();
        let calls = requests.try_iter().collect::<Vec<_>>();
        assert_eq!(calls.len(), 2);
        assert!(calls[1].0.duration_since(calls[0].0) >= Duration::from_millis(950));
        let params = request_params(&calls[1].1);
        assert!(params["query"].starts_with("(arid:00000000-0000-4000-8000-000000000001 OR arid:00000000-0000-4000-8000-000000000002) AND "));
        assert!(params["query"].contains(r#"releasegroup:"Floral LP""#));
        assert!(!params["query"].contains("~2"));
        assert_eq!(params["limit"], "10");
    }
    #[test]
    fn aliases_and_ep_type_arrive_in_two_searches_without_candidate_lookups() {
        let artists = r#"{"count":1,"artists":[{"id":"00000000-0000-4000-8000-000000000002","name":"tsosis","aliases":[{"name":"The Speed of Sound in Seawater"}]}]}"#;
        let groups = r#"{"count":1,"release-groups":[{"id":"00000000-0000-4000-8000-000000000001","title":"Hugs","primary-type":"EP","artist-credit":[{"name":"The Speed of Sound in Seawater","artist":{"id":"00000000-0000-4000-8000-000000000002","name":"tsosis"}}]}]}"#;
        let (mut client, requests, server) =
            mock(vec![(200, artists), (200, groups), (200, groups)]);
        let page = client
            .search_artists("The Speed of Sound in Seawater")
            .unwrap();
        let artist =
            music_library::album_matching::resolve_artist("The Speed of Sound in Seawater", &page)
                .unwrap();
        let page = client.artist_albums(&artist, "Hugs EP").unwrap();
        assert_eq!(page.items[0].primary_type, "EP");
        assert_eq!(page.items[0].artist, "tsosis");
        assert!(matches!(
            music_library::album_matching::accepted_album("Hugs EP", &artist, &page),
            music_library::album_matching::MatchOutcome::MatchedClose(_)
        ));
        client
            .artist_albums_confirmed(&artist, "Nevermimd!")
            .unwrap();
        server.join().unwrap();
        let calls = requests.try_iter().collect::<Vec<_>>();
        assert_eq!(calls.len(), 3);
        let q = request_params(&calls[0].1);
        assert!(q["query"].contains("alias:"));
        let q = request_params(&calls[1].1);
        assert!(q["query"].contains(&format!("arid:{}", artist.external_id)));
        assert!(q["query"].contains(r#"releasegroup:"hugs""#));
        let q = request_params(&calls[2].1);
        assert!(q["query"].contains("~2"));
        for calls in calls.windows(2) {
            assert!(calls[1].0.duration_since(calls[0].0) >= Duration::from_millis(950));
        }
    }
    #[test]
    fn structured_artist_and_scoped_album_search_preserve_rate_and_escape_tags() {
        let artists = r#"{"count":1,"artists":[{"id":"00000000-0000-4000-8000-000000000002","name":"Hella","score":100}]}"#;
        let groups = r#"{"count":1,"release-groups":[{"id":"00000000-0000-4000-8000-000000000001","title":"Acoustics","artist-credit":[{"name":"Hella","artist":{"id":"00000000-0000-4000-8000-000000000002"}}]}]}"#;
        let (mut client, requests, server) = mock(vec![(200, artists), (200, groups)]);
        let page = client.search_artists("Hella").unwrap();
        let artist = &page.items[0].identity;
        let page = client.artist_albums(artist, "Acoustic").unwrap();
        assert_eq!(page.items[0].artist_ids, vec![artist.clone()]);
        assert_eq!(page.items[0].title, "Acoustics");
        server.join().unwrap();
        let calls = requests.try_iter().collect::<Vec<_>>();
        assert_eq!(calls.len(), 2);
        assert!(calls[1].0.duration_since(calls[0].0) >= Duration::from_millis(950));
        let first = request_params(&calls[0].1);
        assert_eq!(first["query"], r#"(artist:"Hella" OR alias:"Hella")"#);
        assert_eq!(first["limit"], "10");
        let second = request_params(&calls[1].1);
        assert_eq!(second["limit"], "10");
        assert_eq!(
            second["query"],
            r#"arid:00000000-0000-4000-8000-000000000002 AND (releasegroup:"Acoustic" OR (releasegroup:Acoustic~1))"#
        );
        assert_eq!(quoted(r#"A" OR *"#), r#""A\" OR \*""#);
    }

    #[test]
    fn recording_discovery_is_one_scoped_request_with_lengths_credits_and_isrcs() {
        let body = r#"{"count":2,"recordings":[{"id":"00000000-0000-4000-8000-000000000001","title":"Song 1","length":100001,"isrcs":["USAAA0100001","USAAA0100002"],"artist-credit":[{"name":"Credited Artist","artist":{"id":"00000000-0000-4000-8000-000000000002","name":"Canonical"}}]},{"id":"00000000-0000-4000-8000-000000000003","title":"Song 2"}]}"#;
        let (mut client, requests, server) = mock(vec![(200, body)]);
        let page = client
            .recordings(&identity(
                "release_group",
                "00000000-0000-4000-8000-000000000004",
            ))
            .unwrap();
        assert_eq!(page.items.len(), 2);
        assert!(page.next_offset.is_none());
        assert_eq!(page.items[0].duration_ms, Some(100001));
        assert_eq!(page.items[0].artist, "Credited Artist");
        assert_eq!(page.items[0].isrcs.len(), 2);
        assert!(page.items[1].isrcs.is_empty());
        assert_eq!(page.items[1].duration_ms, None);
        server.join().unwrap();
        let calls = requests.try_iter().collect::<Vec<_>>();
        assert_eq!(calls.len(), 1);
        let params = request_params(&calls[0].1);
        assert_eq!(params["limit"], "100");
        assert_eq!(params["query"], "rgid:00000000-0000-4000-8000-000000000004");
    }
    #[test]
    fn truncated_recording_search_stays_incomplete() {
        let (mut client, _requests, server) = mock(vec![(200, r#"{"count":101,"recordings":[]}"#)]);
        let page = client
            .recordings(&identity(
                "release_group",
                "00000000-0000-4000-8000-000000000004",
            ))
            .unwrap();
        assert!(page.next_offset.is_some());
        server.join().unwrap();
    }
    #[test]
    #[ignore = "opt-in live request-shape comparison; MUSIC_LIBRARY_RECORDING_GROUP required"]
    fn live_recording_shapes() {
        let group = std::env::var("MUSIC_LIBRARY_RECORDING_GROUP").unwrap();
        mbid(&group).unwrap();
        let titles = std::env::var("MUSIC_LIBRARY_RECORDING_TITLES").unwrap();
        let narrow = format!(
            "rgid:{group} AND ({})",
            titles
                .split('|')
                .map(|t| format!("recording:{}", quoted(t)))
                .collect::<Vec<_>>()
                .join(" OR ")
        );
        let broad = format!("rgid:{group}");
        let client = MusicBrainz::new();
        for round in 0..3 {
            for query in if round % 2 == 0 {
                [&broad, &narrow]
            } else {
                [&narrow, &broad]
            } {
                let start = Instant::now();
                let page: serde_json::Value = client
                    .request(
                        "recording",
                        &[("query", query.clone()), ("limit", "100".into())],
                    )
                    .unwrap();
                println!(
                    "query={query} elapsed={:?} serialized_bytes={} total={} returned={}",
                    start.elapsed(),
                    page.to_string().len(),
                    page["count"],
                    page["recordings"].as_array().unwrap().len()
                );
                if query == &broad {
                    println!("broad candidates={page}");
                }
            }
        }
    }
    #[test]
    fn retry_after_policy_uses_seconds_or_dates_without_sleeping() {
        let now = httpdate::parse_http_date("Tue, 08 Sep 2026 12:00:00 GMT").unwrap();
        assert_eq!(retry_delay(0, None, now), Some(Duration::from_secs(2)));
        assert_eq!(retry_delay(1, None, now), Some(Duration::from_secs(4)));
        assert_eq!(retry_delay(2, Some("0"), now), None);
        assert_eq!(retry_delay(0, Some("7"), now), Some(Duration::from_secs(7)));
        assert_eq!(
            retry_delay(0, Some("Tue, 08 Sep 2026 12:00:09 GMT"), now),
            Some(Duration::from_secs(9))
        );
        assert_eq!(
            retry_delay(0, Some("Tue, 08 Sep 2026 11:59:59 GMT"), now),
            Some(Duration::ZERO)
        );
        assert_eq!(
            retry_delay(0, Some("invalid"), now),
            Some(Duration::from_secs(2))
        );
        assert_eq!(retry_delay(0, Some("61"), now), None); // Never retry sooner than a long server delay.
        assert_eq!(retry_delay(0, Some("999999999999999999999999"), now), None);
    }

    #[test]
    fn transient_503_retries_twice_with_increasing_backoff_then_succeeds() {
        let (mut client, requests, server) =
            mock(vec![(503, "busy"), (503, "busy"), (200, GROUPS)]);
        assert_eq!(client.search_albums("test", 0).unwrap().items.len(), 1);
        server.join().unwrap();
        let calls: Vec<_> = requests.try_iter().collect();
        assert_eq!(calls.len(), 3);
        assert!(calls[1].0.duration_since(calls[0].0) >= Duration::from_millis(1990));
        assert!(calls[2].0.duration_since(calls[1].0) >= Duration::from_millis(3990));
        assert!(calls.windows(2).all(|p| p[0].1 == p[1].1)); // same query, headers and body consumption
    }

    #[test]
    fn retry_after_zero_still_obeys_rate_gate_and_exhaustion_returns_503() {
        let (mut client, requests, server) = mock_with_headers(vec![
            (503, "busy", "Retry-After: 0\r\n"),
            (503, "busy", "Retry-After: 0\r\n"),
            (503, "busy", "Retry-After: 0\r\n"),
            (200, GROUPS, ""),
        ]);
        let failure = client.search_albums("test", 0).unwrap_err();
        assert!(failure.to_string().contains("HTTP 503"));
        assert!(
            matches!(failure, CatalogError::ServiceUnavailable { retry_after: Some(ref v), .. } if v == "0")
        );
        // Another application client shares the same gate after exhaustion.
        let mut another = MusicBrainz::at(client.base.clone());
        another.search_albums("test", 0).unwrap();
        server.join().unwrap();
        let calls: Vec<_> = requests.try_iter().collect();
        assert_eq!(calls.len(), 4);
        assert!(
            calls
                .windows(2)
                .all(|p| p[1].0.duration_since(p[0].0) >= Duration::from_millis(990))
        );
    }

    #[test]
    fn search_uses_ten_results_and_preserves_actual_count_pagination() {
        let mut first: serde_json::Value = serde_json::from_str(GROUPS).unwrap();
        let template = first["release-groups"][0].clone();
        let groups: Vec<_> = (0..11)
            .map(|i| {
                let mut group = template.clone();
                group["id"] = format!("00000000-0000-4000-8000-{i:012}").into();
                group
            })
            .collect();
        first["count"] = 11.into();
        first["release-groups"] = serde_json::json!(&groups[..10]);
        let mut second = first.clone();
        second["release-groups"] = serde_json::json!(&groups[10..]);
        let first = first.to_string();
        let second = second.to_string();
        let (mut client, requests, server) = mock(vec![(200, &first), (200, &second)]);
        let page = client
            .search_albums("Animals AND artist:Pink Floyd", 0)
            .unwrap();
        assert_eq!(page.items.len(), 10);
        assert_eq!(page.next_offset, Some(10));
        let last = client
            .search_albums("Animals AND artist:Pink Floyd", page.next_offset.unwrap())
            .unwrap();
        assert_eq!(last.items.len(), 1);
        assert_eq!(last.next_offset, None);
        assert!(
            !page
                .items
                .iter()
                .any(|a| a.identity == last.items[0].identity)
        );
        server.join().unwrap();
        let calls: Vec<_> = requests.try_iter().collect();
        assert_eq!(calls.len(), 2);
        for (index, (_, request)) in calls.iter().enumerate() {
            let params = request_params(request);
            assert_eq!(params["limit"], "10");
            assert_eq!(params["offset"], (index * 10).to_string());
            assert_eq!(params["query"], "Animals AND artist:Pink Floyd");
        }
    }

    #[test]
    fn request_flow_parsing_rate_limit_and_atomic_import() {
        let (mut client, requests, server) =
            mock(vec![(200, GROUPS), (200, EDITIONS), (200, RELEASE)]);
        let groups = client.search_albums("Album & artist:雪", 0).unwrap();
        assert_eq!(groups.next_offset, Some(1));
        let g = &groups.items[0];
        assert_eq!(g.artist, "Artist A feat. Artist B");
        assert_eq!(g.date, "2001-03");
        assert_eq!(g.primary_type, "Album");
        assert_eq!(g.secondary_types, ["Live"]);
        let editions = client.releases(&g.identity, 0).unwrap();
        assert_eq!(editions.items.len(), 2);
        assert_eq!(editions.items[0].track_count, 3);
        assert_eq!(editions.items[0].labels, ["Label CAT-1"]);
        assert_eq!(editions.items[1].country, "");
        let release = client.release(&editions.items[0].identity).unwrap();
        assert_eq!(
            release.media.iter().map(|m| m.position).collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(release.media[0].tracks[0].title, "Song Title (Live)");
        assert_eq!(release.media[0].tracks[1].title, "Song Title - Remix");
        assert_eq!(
            catalog::credit_display(&release.media[0].tracks[0].credits),
            "Artist A feat. Artist B"
        );
        assert_eq!(
            catalog::credit_display(&release.media[0].tracks[1].credits),
            "Recording Artist"
        );
        assert_eq!(
            catalog::credit_display(&release.media[1].tracks[0].credits),
            "Artist A feat. Artist B"
        );
        let ids = &release.media[0].tracks[0].identities;
        assert_ne!(ids[0].external_id, ids[1].external_id);
        assert_eq!(ids.iter().filter(|id| id.provider == "isrc").count(), 2);
        server.join().unwrap();
        let calls: Vec<_> = requests.try_iter().collect();
        assert_eq!(calls.len(), 3);
        for pair in calls.windows(2) {
            assert!(pair[1].0.duration_since(pair[0].0) >= Duration::from_millis(990));
        }
        let urls: Vec<_> = calls
            .iter()
            .map(|(_, r)| {
                assert!(r.contains(USER_AGENT));
                Url::parse(&format!(
                    "http://localhost{}",
                    r.split_whitespace().nth(1).unwrap()
                ))
                .unwrap()
            })
            .collect();
        assert_eq!(
            urls[0].query_pairs().find(|(k, _)| k == "query").unwrap().1,
            "Album & artist:雪"
        );
        assert!(
            urls[1]
                .query_pairs()
                .any(|(k, v)| k == "limit" && v == "100")
        );
        assert!(
            urls[1]
                .query_pairs()
                .any(|(k, v)| k == "release-group" && v == g.identity.external_id)
        );
        assert!(
            urls[2]
                .query_pairs()
                .any(|(k, v)| k == "inc" && v.contains("isrcs") && v.contains("recordings"))
        );
        let mut library = music_library::Library::open_in_memory().unwrap();
        let imported = library.add_catalog_release(&release).unwrap();
        let album = library.album_for_release(&imported.release_id).unwrap();
        assert_eq!(album.title, "Album (Live)");
        assert_eq!(
            library
                .resolve_albums_external_identity(&g.identity)
                .unwrap(),
            vec![album.album_id]
        );
        assert!(
            library
                .resolve_releases_external_identity(&g.identity)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            library
                .list_release_external_identities(&imported.release_id)
                .unwrap(),
            vec![release.identity.clone()]
        );
        assert_eq!(imported.track_ids.len(), 3);
        assert_eq!(library.add_catalog_release(&release).unwrap(), imported);
    }
    #[test]
    fn pregap_and_data_tracks_are_not_lost_when_regular_tracks_are_absent() {
        // MusicBrainz serializes pregap and data tracks separately from track-count/tracks.
        let mut value: serde_json::Value = serde_json::from_str(RELEASE).unwrap();
        let medium = &mut value["media"][0];
        let mut pregap = medium["tracks"][0].clone();
        pregap["id"] = serde_json::json!("00000000-0000-0000-0000-000000000099");
        pregap["position"] = serde_json::json!(0);
        medium["pregap"] = pregap;
        medium["data-tracks"] = medium["tracks"].clone();
        medium["track-count"] = serde_json::json!(0);
        medium.as_object_mut().unwrap().remove("tracks");
        let release = convert_release(serde_json::from_value(value).unwrap()).unwrap();
        assert_eq!(
            release.media[1]
                .tracks
                .iter()
                .map(|t| t.position)
                .collect::<Vec<_>>(),
            [0, 1]
        );
    }

    fn request_params(request: &str) -> std::collections::HashMap<String, String> {
        let target = request.split_whitespace().nth(1).unwrap();
        Url::parse(&format!("http://localhost{target}"))
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect()
    }

    #[test]
    fn default_candidates_are_light_official_and_keep_media_ranking_and_paging() {
        let mut fixture: serde_json::Value = serde_json::from_str(EDITIONS).unwrap();
        fixture["release-count"] = 103.into(); // simulate a page capped below limit=100
        for release in fixture["releases"].as_array_mut().unwrap() {
            release.as_object_mut().unwrap().remove("artist-credit");
            release.as_object_mut().unwrap().remove("label-info");
        }
        let fixture = fixture.to_string();
        let (mut client, requests, server) =
            mock(vec![(200, &fixture), (200, EDITIONS), (200, RELEASE)]);
        let album = identity("release_group", "00000000-0000-4000-8000-000000000001");
        let page = client.representative_releases(&album).unwrap();
        assert_eq!(page.next_offset, Some(2));
        assert_eq!(page.items[0].disc_count, 2);
        assert_eq!(page.items[0].track_count, 3);
        assert!(page.items[0].artist.is_empty());
        assert!(page.items[0].labels.is_empty());
        // An explicit Editions request is still unfiltered and rich, with actual offset.
        let rich = client.releases(&album, 2).unwrap();
        assert_eq!(rich.items[0].labels, ["Label CAT-1"]);
        client.release(&page.items[0].identity).unwrap();
        server.join().unwrap();
        let calls: Vec<_> = requests.try_iter().collect();
        assert_eq!(calls.len(), 3);
        let first = request_params(&calls[0].1);
        assert_eq!(first["release-group"], album.external_id);
        assert_eq!(first["status"], "official");
        assert_eq!(first["inc"], "media");
        assert_eq!(first["limit"], "100");
        let second = request_params(&calls[1].1);
        assert!(!second.contains_key("status"));
        assert_eq!(second["inc"], "artist-credits+labels+media");
        assert_eq!(second["offset"], "2");
        assert_eq!(
            request_params(&calls[2].1)["inc"],
            "release-groups+recordings+artist-credits+isrcs+media"
        );
        assert!(
            calls
                .windows(2)
                .all(|p| p[1].0.duration_since(p[0].0) >= Duration::from_millis(990))
        );
    }

    #[test]
    fn no_official_candidates_falls_back_once_without_rich_expansions() {
        let mut fixture: serde_json::Value = serde_json::from_str(EDITIONS).unwrap();
        fixture["releases"][0]["status"] = "Bootleg".into();
        let fixture = fixture.to_string();
        let empty = r#"{"release-count":0,"releases":[]}"#;
        let (mut client, requests, server) = mock(vec![(200, empty), (200, &fixture)]);
        let page = client
            .representative_releases(&identity(
                "release_group",
                "00000000-0000-4000-8000-000000000001",
            ))
            .unwrap();
        assert_eq!(page.items[0].status, "Bootleg");
        server.join().unwrap();
        let calls: Vec<_> = requests.try_iter().collect();
        assert_eq!(calls.len(), 2);
        assert_eq!(request_params(&calls[0].1)["status"], "official");
        assert!(!request_params(&calls[1].1).contains_key("status"));
        assert_eq!(request_params(&calls[1].1)["inc"], "media");
        assert!(calls[1].0.duration_since(calls[0].0) >= Duration::from_millis(990));
    }

    #[test]
    fn candidate_failure_is_not_mistaken_for_no_official_releases() {
        let (mut client, requests, server) = mock(vec![(400, "bad request")]);
        assert!(
            client
                .representative_releases(&identity(
                    "release_group",
                    "00000000-0000-4000-8000-000000000001"
                ))
                .unwrap_err()
                .to_string()
                .contains("HTTP 400")
        );
        server.join().unwrap();
        assert_eq!(requests.try_iter().count(), 1);
    }

    #[test]
    fn http_malformed_incomplete_and_identity_errors_are_recoverable() {
        let (mut client, requests, server) = mock(vec![
            (400, "malformed query"),
            (429, "slow down"),
            (200, "not JSON"),
            (200, "{}"),
        ]);
        for _ in 0..4 {
            assert!(client.search_albums("test", 0).is_err());
        }
        server.join().unwrap();
        assert_eq!(requests.try_iter().count(), 4); // no automatic retry
        assert!(client.release(&identity("release", "../bad")).is_err());
        let mut value: serde_json::Value = serde_json::from_str(RELEASE).unwrap();
        value["media"][0]["track-count"] = serde_json::json!(5);
        assert!(convert_release(serde_json::from_value(value).unwrap()).is_err());
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        drop(socket);
        assert!(
            MusicBrainz::at(Url::parse(&format!("http://{address}/")).unwrap())
                .search_albums("test", 0)
                .is_err()
        );
    }
}

#[cfg(test)]
#[path = "request_shape_audit.rs"]
mod request_shape_audit;
