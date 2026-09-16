//! Spotify catalog client and independent user playback adapter. Provider JSON
//! and credentials stay here; catalog Client Credentials never authorize playback.
pub mod playback;
use base64::{Engine, engine::general_purpose::STANDARD};
use music_library::{
    album_program::{Program, Programs},
    catalog::*,
    domain::ExternalIdentity,
    edition::{ArtistEvidence, RecordingEvidence, TrackEvidence},
};
use serde::{Deserialize, de::DeserializeOwned};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use url::Url;

pub fn matching_scope() -> MatchingScope {
    MatchingScope {
        provider: "spotify".into(),
        artist_kind: "artist".into(),
        album_kind: "album".into(),
    }
}
fn id(kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "spotify".into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
fn config(message: &str) -> CatalogError {
    CatalogError::Configuration {
        status: 0,
        message: message.into(),
    }
}
pub struct Config {
    client_id: String,
    client_secret: String,
    market: String,
}
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpotifyConfig")
            .field("credentials", &"[REDACTED]")
            .field("market", &self.market)
            .finish()
    }
}
impl Config {
    pub fn new(
        client_id: String,
        client_secret: String,
        market: String,
    ) -> Result<Self, CatalogError> {
        if client_id.trim().is_empty() || client_secret.trim().is_empty() {
            return Err(config("Set SPOTIFY_CLIENT_ID and SPOTIFY_CLIENT_SECRET"));
        }
        let market = market.to_ascii_uppercase();
        if market.len() != 2 || !market.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(config(
                "SPOTIFY_MARKET must be an explicit ISO two-letter country code, e.g. US",
            ));
        }
        Ok(Self {
            client_id,
            client_secret,
            market,
        })
    }
    pub fn from_env() -> Result<Self, CatalogError> {
        let read = |name| {
            std::env::var(name)
                .map_err(|_| config(&format!("Set {name} for Spotify catalog matching")))
        };
        Self::new(
            read("SPOTIFY_CLIENT_ID")?,
            read("SPOTIFY_CLIENT_SECRET")?,
            read("SPOTIFY_MARKET")?,
        )
    }
}
struct Token {
    value: String,
    expires: Instant,
}
pub struct Spotify {
    config: Config,
    agent: ureq::Agent,
    base: Url,
    token_url: Url,
    token: Option<Token>,
    artist_names: HashMap<String, String>,
    programs: Vec<Programs>,
    configuration_error: Option<CatalogError>,
    candidate_counts: HashMap<String, u32>,
    request_counts: (u64, u64),
}
impl std::fmt::Debug for Spotify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spotify")
            .field("config", &self.config)
            .field("token", &"[REDACTED]")
            .finish()
    }
}
impl Spotify {
    pub fn new(config: Config) -> Self {
        Self::at(
            config,
            Url::parse("https://api.spotify.com/v1/").unwrap(),
            Url::parse("https://accounts.spotify.com/api/token").unwrap(),
        )
    }
    pub fn from_env() -> Result<Self, CatalogError> {
        Ok(Self::new(Config::from_env()?))
    }
    fn at(config: Config, base: Url, token_url: Url) -> Self {
        Self {
            config,
            base,
            token_url,
            token: None,
            artist_names: HashMap::new(),
            programs: vec![],
            configuration_error: None,
            candidate_counts: HashMap::new(),
            request_counts: (0, 0),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(30)))
                .max_redirects(0)
                .http_status_as_error(false)
                .build()
                .into(),
        }
    }
    fn sanitize(&self, text: &str) -> String {
        let mut result = text
            .replace(&self.config.client_secret, "[REDACTED]")
            .replace(&self.config.client_id, "[REDACTED]");
        if let Some(token) = &self.token {
            result = result.replace(&token.value, "[REDACTED]");
        }
        result.chars().take(400).collect()
    }
    /// Catalog token and API HTTP requests, excluding the independent playback client.
    pub fn request_counts(&self) -> (u64, u64) {
        self.request_counts
    }
    fn response<T: DeserializeOwned>(
        &self,
        mut response: ureq::http::Response<ureq::Body>,
        auth: bool,
    ) -> Result<T, CatalogError> {
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        Timing::event(format_args!(
            "spotify status={status} token_endpoint={auth}"
        ));
        let body = response
            .body_mut()
            .with_config()
            .limit(8 * 1024 * 1024)
            .read_to_string()
            .map_err(transport)?;
        if !(200..300).contains(&status) {
            let value: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let reason = value
                .pointer("/error/reason")
                .or_else(|| value.get("reason"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let detail = if auth {
                "Check client credentials, app access and Premium eligibility"
            } else {
                value
                    .pointer("/error/message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Catalog request failed")
            };
            let message =
                self.sanitize(&format!("Spotify HTTP {status}: {detail}; reason={reason}"));
            return Err(match status {
                429 => CatalogError::RateLimited {
                    status,
                    message,
                    retry_after,
                },
                500..=599 => CatalogError::ServiceUnavailable {
                    message,
                    retry_after,
                },
                401 | 403 => CatalogError::Configuration { status, message },
                400 if auth => CatalogError::Configuration { status, message },
                _ => CatalogError::Other(message),
            });
        }
        // Never include response contents/serde errors: token payloads are secrets.
        serde_json::from_str(&body)
            .map_err(|_| CatalogError::Other("Invalid Spotify JSON response".into()))
    }
    fn token(&mut self) -> Result<&str, CatalogError> {
        if let Some(error) = &self.configuration_error {
            return Err(error.clone());
        }
        if self
            .token
            .as_ref()
            .is_none_or(|t| Instant::now() >= t.expires)
        {
            #[derive(Deserialize)]
            struct Auth {
                access_token: String,
                expires_in: u64,
                token_type: String,
            }
            let _timing = Timing::new("spotify.token_acquisition");
            let encoded = STANDARD.encode(format!(
                "{}:{}",
                self.config.client_id, self.config.client_secret
            ));
            self.request_counts.0 += 1;
            let response = self
                .agent
                .post(self.token_url.as_str())
                .header("Authorization", &format!("Basic {encoded}"))
                .header("Content-Type", "application/x-www-form-urlencoded")
                .send("grant_type=client_credentials")
                .map_err(transport)?;
            let auth: Auth = match self.response(response, true) {
                Err(error @ CatalogError::Configuration { .. }) => {
                    self.configuration_error = Some(error.clone());
                    return Err(error);
                }
                other => other?,
            };
            if auth.access_token.is_empty()
                || !auth.token_type.eq_ignore_ascii_case("bearer")
                || auth.expires_in == 0
            {
                return Err(config("Invalid Spotify token response"));
            }
            let lifetime = auth.expires_in.min(86400);
            let margin = 30.min(lifetime / 10);
            self.token = Some(Token {
                value: auth.access_token,
                expires: Instant::now() + Duration::from_secs(lifetime - margin),
            });
        }
        Ok(&self.token.as_ref().unwrap().value)
    }
    fn get<T: DeserializeOwned>(
        &mut self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T, CatalogError> {
        let mut url = self
            .base
            .join(path)
            .map_err(|_| config("Invalid Spotify endpoint"))?;
        url.query_pairs_mut()
            .append_pair("market", &self.config.market)
            .extend_pairs(params.iter().map(|(k, v)| (*k, v.as_str())));
        for retry in 0..=1 {
            let token = self.token()?.to_owned();
            let _timing = Timing::new(format!("spotify.{path}.http"));
            self.request_counts.1 += 1;
            let response = self
                .agent
                .get(url.as_str())
                .header("Authorization", &format!("Bearer {token}"))
                .call()
                .map_err(transport)?;
            if response.status().as_u16() == 401 && retry == 0 {
                self.token = None;
                continue;
            }
            let result = self.response(response, false);
            if let Err(error @ CatalogError::Configuration { .. }) = &result {
                self.configuration_error = Some(error.clone());
            }
            return result;
        }
        unreachable!()
    }
    fn require(identity: &ExternalIdentity, kind: &str) -> Result<(), CatalogError> {
        if identity.provider != "spotify"
            || identity.kind != kind
            || identity.external_id.is_empty()
            || !identity
                .external_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric())
        {
            return Err(config("Invalid Spotify catalog identity"));
        }
        Ok(())
    }
    fn artist_name(&mut self, artist: &ExternalIdentity) -> Result<String, CatalogError> {
        Self::require(artist, "artist")?;
        if let Some(name) = self.artist_names.get(&artist.external_id) {
            return Ok(name.clone());
        }
        let found: Artist = self.get(&format!("artists/{}", artist.external_id), &[])?;
        if found.id != artist.external_id {
            return Err(config("Spotify returned another Artist"));
        }
        self.remember_artist(&found);
        Ok(found.name)
    }
    fn remember_artist(&mut self, a: &Artist) {
        if self.artist_names.len() >= 64 {
            self.artist_names.clear();
        }
        self.artist_names.insert(a.id.clone(), a.name.clone());
    }
    fn scoped_albums(
        &mut self,
        artists: &[ExternalIdentity],
        title: &str,
    ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
        if artists.is_empty() || artists.len() > 10 {
            return Err(config("Expected bounded Artist candidates"));
        }
        let mut names = vec![];
        for artist in artists {
            let name = self.artist_name(artist)?;
            if !names.contains(&name) {
                names.push(name);
            }
        }
        // Spotify has no arid search field. Search with a name only when all
        // plausible Artists share it, then enforce membership using returned IDs.
        let base = music_library::album_matching::album_title_variants(title)
            .first()
            .map(|v| v.title.clone())
            .unwrap_or_else(|| title.to_owned());
        let mut query = format!("album:{}", quoted(&base));
        if let [name] = names.as_slice() {
            query.push_str(&format!(" artist:{}", quoted(name)));
        }
        Timing::event(format_args!("spotify album query={query:?}"));
        let mut response: AlbumSearch = self.get(
            "search",
            &[
                ("q", query),
                ("type", "album".into()),
                ("limit", "10".into()),
                ("offset", "0".into()),
            ],
        )?;
        // Spotify can return no results for a quoted full title it actually
        // indexes. One distinctive token is discovery only: returned Artist IDs
        // and the complete original title still pass the unchanged core matcher.
        if response.albums.items.is_empty()
            && !response.albums.more()
            && names.len() == 1
            && base.split_whitespace().count() > 1
            && let Some(token) = base
                .split(|c: char| !c.is_alphanumeric())
                .filter(|s| s.chars().count() >= 4)
                .max_by_key(|s| s.chars().count())
        {
            let fallback = format!("album:{} artist:{}", quoted(token), quoted(&names[0]));
            Timing::event(format_args!(
                "spotify empty-title discovery fallback={fallback:?}"
            ));
            response = self.get(
                "search",
                &[
                    ("q", fallback),
                    ("type", "album".into()),
                    ("limit", "10".into()),
                    ("offset", "0".into()),
                ],
            )?;
        }
        let more = response.albums.more();
        self.candidate_counts.clear();
        Timing::event(format_args!(
            "spotify album page total={} more={more}",
            response.albums.total
        ));
        for a in &response.albums.items {
            if let Some(count) = a.total_tracks {
                self.candidate_counts.insert(a.id.clone(), count);
            }
            Timing::event(format_args!(
                "spotify album candidate id={} title={:?} artist={:?} type={} total_tracks={:?} date={} artist_scope={}",
                a.id,
                a.name,
                display(&a.artists),
                a.album_type,
                a.total_tracks,
                a.release_date,
                a.artists
                    .iter()
                    .any(|a| artists.contains(&id("artist", &a.id)))
            ));
        }
        let items = response
            .albums
            .items
            .into_iter()
            .filter(|a| {
                a.artists
                    .iter()
                    .any(|a| artists.contains(&id("artist", &a.id)))
            })
            .map(|a| ArtistAlbumCandidate {
                identity: id("album", &a.id),
                title: a.name,
                artist: display(&a.artists),
                artist_ids: a.artists.iter().map(|a| id("artist", &a.id)).collect(),
                date: a.release_date,
                primary_type: match a.album_type.as_str() {
                    "album" => "Album",
                    "compilation" => "Compilation",
                    "single" => "Single",
                    _ => "",
                }
                .into(),
                comment: String::new(),
            })
            .collect();
        Ok(Page {
            items,
            next_offset: more.then_some(10),
        })
    }
}
fn transport(error: ureq::Error) -> CatalogError {
    match error {
        ureq::Error::Timeout(_) => CatalogError::Timeout("Spotify request timed out".into()),
        _ => CatalogError::TransportUnavailable("Spotify network/transport request failed".into()),
    }
}
fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
fn display(artists: &[Artist]) -> String {
    artists
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
#[derive(Deserialize)]
struct Artist {
    id: String,
    name: String,
}
#[derive(Deserialize)]
struct Album {
    id: String,
    name: String,
    artists: Vec<Artist>,
    #[serde(default)]
    release_date: String,
    #[serde(default)]
    album_type: String,
    #[serde(default)]
    total_tracks: Option<u32>,
}
#[derive(Deserialize)]
struct Paging<T> {
    items: Vec<T>,
    total: usize,
    #[serde(default)]
    offset: usize,
    #[serde(default)]
    next: Option<String>,
}
impl<T> Paging<T> {
    fn more(&self) -> bool {
        self.next.is_some() || self.offset + self.items.len() < self.total
    }
}
#[derive(Deserialize)]
struct ArtistSearch {
    artists: Paging<Artist>,
}
#[derive(Deserialize)]
struct AlbumSearch {
    albums: Paging<Album>,
}
#[derive(Deserialize)]
struct Song {
    id: Option<String>,
    name: String,
    disc_number: u32,
    track_number: u32,
    duration_ms: u64,
    artists: Vec<Artist>,
    #[serde(default)]
    external_ids: HashMap<String, String>,
    #[serde(default)]
    is_local: bool,
}
#[derive(Deserialize)]
struct SearchSong {
    #[serde(flatten)]
    song: Song,
    album: Album,
}
#[derive(Deserialize)]
struct SongSearch {
    tracks: Paging<SearchSong>,
}
impl music_library::song_resolution::SongSearch for Spotify {
    fn search_songs(
        &mut self,
        input: &music_library::song_resolution::Input,
    ) -> Result<Page<music_library::song_resolution::Candidate>, CatalogError> {
        if input.title.trim().is_empty() || input.artist.trim().is_empty() {
            return Err(CatalogError::Other(
                "Song resolution needs a Track title and credited Artist".into(),
            ));
        }
        // Native search shares the existing catalog get/token/market/error path.
        // Album remains presentation evidence rather than an exact-edition filter.
        let query = format!(
            "track:{} artist:{}",
            quoted(&input.title),
            quoted(&input.artist)
        );
        let response: SongSearch = self.get(
            "search",
            &[
                ("q", query),
                ("type", "track".into()),
                ("limit", "10".into()),
                ("offset", "0".into()),
            ],
        )?;
        let more = response.tracks.more();
        let mut seen = std::collections::HashSet::new();
        let items = response
            .tracks
            .items
            .into_iter()
            .take(10)
            .filter_map(|entry| {
                let s = entry.song;
                let key = s.id?;
                if s.is_local
                    || key.len() != 22
                    || !key.bytes().all(|b| b.is_ascii_alphanumeric())
                    || !seen.insert(key.clone())
                {
                    return None;
                }
                Some(music_library::song_resolution::Candidate {
                    identity: id("track", &key),
                    title: s.name,
                    artist: display(&s.artists),
                    album: entry.album.name,
                    date: entry.album.release_date,
                    duration_ms: s.duration_ms,
                    disc: s.disc_number,
                    number: s.track_number,
                })
            })
            .collect();
        Ok(Page {
            items,
            next_offset: more.then_some(10),
        })
    }
}
impl CatalogProvider for Spotify {
    fn album_candidate_programs(&self) -> bool {
        true
    }
    fn album_candidate_track_count(&self, album: &ExternalIdentity) -> Option<u32> {
        (album.provider == "spotify" && album.kind == "album")
            .then(|| self.candidate_counts.get(&album.external_id).copied())
            .flatten()
    }
    fn album_program_namespaces(&self) -> Vec<(String, String)> {
        vec![("spotify".into(), "album".into())]
    }
    fn search_artists(&mut self, name: &str) -> Result<Page<ArtistCandidate>, CatalogError> {
        let response: ArtistSearch = self.get(
            "search",
            &[
                ("q", format!("artist:{}", quoted(name))),
                ("type", "artist".into()),
                ("limit", "10".into()),
                ("offset", "0".into()),
            ],
        )?;
        let more = response.artists.more();
        Timing::event(format_args!(
            "spotify artist query={name:?} total={} more={more}",
            response.artists.total
        ));
        let mut items = vec![];
        for a in response.artists.items {
            Timing::event(format_args!(
                "spotify artist candidate id={} name={:?}",
                a.id, a.name
            ));
            self.remember_artist(&a);
            items.push(ArtistCandidate {
                identity: id("artist", &a.id),
                name: a.name,
                aliases: vec![],
                comment: String::new(),
                country: String::new(),
                artist_type: String::new(),
                score: None,
            });
        }
        Ok(Page {
            items,
            next_offset: more.then_some(10),
        })
    }
    fn artist_albums(
        &mut self,
        artist: &ExternalIdentity,
        title: &str,
    ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
        self.scoped_albums(std::slice::from_ref(artist), title)
    }
    fn albums_for_artists(
        &mut self,
        artists: &[ExternalIdentity],
        title: &str,
    ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
        self.scoped_albums(artists, title)
    }
    fn album_programs(&mut self, album: &ExternalIdentity) -> Result<Programs, CatalogError> {
        Self::require(album, "album")?;
        if let Some(p) = self.programs.iter().find(|p| &p.album == album) {
            return Ok(p.clone());
        }
        let mut tracks = vec![];
        let mut complete = false;
        let mut offset = 0;
        let mut expected = None;
        // 20 pages / 1000 entries maximum, independent of local Track count.
        for _ in 0..20 {
            let response: Paging<Song> = self.get(
                &format!("albums/{}/tracks", album.external_id),
                &[("limit", "50".into()), ("offset", offset.to_string())],
            )?;
            if response.offset != offset || expected.is_some_and(|n| n != response.total) {
                return Err(CatalogError::Other(
                    "Spotify Album program changed during pagination".into(),
                ));
            }
            expected = Some(response.total);
            let more = response.more();
            let count = response.items.len();
            if count == 0 && more {
                return Err(CatalogError::Other("Incomplete Spotify Album page".into()));
            }
            for song in response.items {
                let identity = song.id.filter(|id| !id.is_empty());
                if identity.is_none() || song.is_local {
                    return Err(CatalogError::Other(
                        "Unavailable Spotify Album Track identity".into(),
                    ));
                }
                tracks.push(TrackEvidence {
                    identities: vec![id("track", &identity.unwrap())],
                    title: Some(song.name),
                    disc: Some(song.disc_number),
                    number: Some(song.track_number),
                    duration_ms: Some(song.duration_ms),
                    artists: song
                        .artists
                        .iter()
                        .enumerate()
                        .map(|(i, a)| ArtistEvidence {
                            name: a.name.clone(),
                            identities: vec![id("artist", &a.id)],
                            join_phrase: if i + 1 < song.artists.len() {
                                ", ".into()
                            } else {
                                String::new()
                            },
                        })
                        .collect(),
                    recording: RecordingEvidence {
                        identities: vec![],
                        isrcs: song.external_ids.get("isrc").cloned().into_iter().collect(),
                    },
                });
            }
            offset += count;
            if !more {
                complete = true;
                break;
            }
        }
        let p = Programs {
            album: album.clone(),
            programs: vec![Program {
                identity: None,
                tracks,
                complete,
            }],
            note: format!(
                "Spotify market {}; catalog song occurrences, no Recording or exact edition claim",
                self.config.market
            ),
        };
        if complete {
            if self.programs.len() == 4 {
                self.programs.remove(0);
            }
            self.programs.push(p.clone());
        }
        Ok(p)
    }
    fn search_albums(
        &mut self,
        query: &str,
        offset: u32,
    ) -> Result<Page<AlbumCandidate>, CatalogError> {
        let response: AlbumSearch = self.get(
            "search",
            &[
                ("q", query.into()),
                ("type", "album".into()),
                ("limit", "10".into()),
                ("offset", offset.to_string()),
            ],
        )?;
        let more = response.albums.more();
        Ok(Page {
            items: response
                .albums
                .items
                .into_iter()
                .map(|a| AlbumCandidate {
                    identity: id("album", &a.id),
                    title: a.name,
                    artist: display(&a.artists),
                    date: a.release_date,
                    credits: a
                        .artists
                        .into_iter()
                        .map(|a| Credit {
                            identity: Some(id("artist", &a.id)),
                            name: a.name,
                            join_phrase: String::new(),
                        })
                        .collect(),
                    primary_type: a.album_type,
                    secondary_types: vec![],
                    comment: String::new(),
                    score: None,
                })
                .collect(),
            next_offset: more.then_some(offset + 10),
        })
    }
    fn releases(
        &mut self,
        _: &ExternalIdentity,
        _: u32,
    ) -> Result<Page<ReleaseCandidate>, CatalogError> {
        Err(CatalogError::Other(
            "Spotify edition browsing is not part of Album matching".into(),
        ))
    }
    fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
        Err(CatalogError::Other(
            "Spotify catalog Add Album is not implemented; local Album matching is supported"
                .into(),
        ))
    }
}

#[cfg(test)]
mod tests;
