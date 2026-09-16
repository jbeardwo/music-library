//! User-authorized Spotify Connect control. Independent from catalog credentials,
//! library identity, local decoding, and the application's playback queue.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    digest,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use url::Url;

pub const REDIRECT: &str = "http://127.0.0.1:43821/callback";
pub const SCOPES: &str = "user-read-playback-state user-modify-playback-state";
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    AuthorizationRequired,
    ReauthorizationRequired,
    Denied,
    InvalidState,
    InsufficientScope,
    CapabilityUnavailable,
    NoDevice,
    DeviceUnavailable,
    RestrictedDevice,
    RateLimited(u64),
    ServiceUnavailable,
    Transport,
    ApiRejected(u16),
    NoAssociation,
    Storage,
    Configuration,
    InvalidResponse,
    CallbackUnavailable,
    AuthorizationTimedOut,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::NoAssociation => "No Spotify playback association",
            Self::AuthorizationRequired => "Connect Spotify Playback first",
            Self::ReauthorizationRequired => {
                "Spotify playback authorization expired or was revoked; connect again"
            }
            Self::NoDevice => {
                "Select a Spotify device; if none appear, open Spotify on this computer and refresh devices"
            }
            Self::DeviceUnavailable => {
                "Selected Spotify device unavailable; refresh and select the desktop client again"
            }
            Self::RestrictedDevice => {
                "Selected Spotify device is restricted and cannot be controlled"
            }
            Self::CapabilityUnavailable => {
                "Spotify Premium/playback capability unavailable for this account"
            }
            Self::InsufficientScope => {
                "Spotify playback permissions are insufficient; connect again"
            }
            Self::Configuration => {
                "Spotify playback configuration error; check SPOTIFY_CLIENT_ID and the registered redirect URI"
            }
            Self::Storage => {
                "Cannot securely read/write the separate Spotify playback credential file; check its path and permissions"
            }
            Self::Denied => "Spotify playback authorization was denied",
            Self::CallbackUnavailable => {
                "Cannot receive Spotify callback on 127.0.0.1:43821; check for another authorization listener"
            }
            _ => return write!(f, "Spotify playback: {self:?}"),
        };
        f.write_str(message)
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AuthorizationState {
    #[default]
    Disconnected,
    Authorizing,
    Connected,
    ReauthorizationRequired,
}
#[derive(Clone, Debug, Deserialize, Default)]
pub struct Device {
    pub id: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub is_active: bool,
    pub is_restricted: bool,
    #[serde(default)]
    pub supports_volume: bool,
}
#[derive(Clone, Debug, Default)]
pub struct State {
    pub track_id: Option<String>,
    pub title: String,
    pub progress_ms: u64,
    pub playing: bool,
    pub device: Option<Device>,
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub authorization: AuthorizationState,
    pub devices: Vec<Device>,
    pub selected_device: Option<String>,
    pub state: State,
    pub error: Option<Error>,
    pub authorization_url: Option<String>,
    pub token_requests: u64,
    pub api_requests: u64,
    pub token_http_ms: u128,
    pub api_http_ms: u128,
    pub last_http_status: Option<u16>,
}

// Deliberately no Debug: these types contain credentials or a transient verifier.
#[derive(Serialize, Deserialize)]
struct Tokens {
    client_id: String,
    access_token: String,
    refresh_token: String,
    access_expires_at: u64,
    authorized_at: u64,
}
pub struct Authorization {
    verifier: String,
    state: String,
    listener: TcpListener,
    started: Instant,
}
fn random() -> Result<String> {
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::Configuration)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(digest::digest(&digest::SHA256, verifier.as_bytes()))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// An adapter-owned validated URI; callers supply persisted occurrence evidence,
/// never a filename, arbitrary URI, or a Recording identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Song(String);
impl Song {
    pub fn from_associations(ids: &[music_library::domain::ExternalIdentity]) -> Result<Self> {
        let mut values: Vec<_> = ids
            .iter()
            .filter(|i| i.provider == "spotify" && i.kind == "track")
            .map(|i| i.external_id.as_str())
            .collect();
        values.sort_unstable();
        values.dedup();
        if values.len() != 1
            || values[0].len() != 22
            || !values[0].bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return Err(Error::NoAssociation);
        }
        Ok(Self(values[0].into()))
    }
    pub fn uri(&self) -> String {
        format!("spotify:track:{}", self.0)
    }
}

pub struct Playback {
    client_id: String,
    file: PathBuf,
    tokens: Option<Tokens>,
    agent: ureq::Agent,
    token_url: String,
    base: Url,
    not_before: Instant,
    pub snapshot: Snapshot,
}
impl std::fmt::Debug for Playback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpotifyPlayback")
            .field("credentials", &"[REDACTED]")
            .finish()
    }
}
impl Playback {
    pub fn from_env() -> Result<Self> {
        let client = std::env::var("SPOTIFY_CLIENT_ID").map_err(|_| Error::Configuration)?;
        let root = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .ok_or(Error::Configuration)?;
        let file = std::env::var_os("MUSIC_LIBRARY_SPOTIFY_PLAYBACK_CREDENTIALS")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("music-library/spotify-playback.json"));
        Self::new(client, file)
    }
    pub fn new(client_id: String, file: PathBuf) -> Result<Self> {
        if client_id.trim().is_empty() {
            return Err(Error::Configuration);
        }
        let tokens: Option<Tokens> = match fs::read(&file) {
            Ok(bytes) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if fs::metadata(&file)
                        .map_err(|_| Error::Storage)?
                        .permissions()
                        .mode()
                        & 0o077
                        != 0
                    {
                        return Err(Error::Storage);
                    }
                }
                Some(serde_json::from_slice(&bytes).map_err(|_| Error::Storage)?)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(Error::Storage),
        };
        let tokens = tokens.filter(|t| t.client_id == client_id);
        let connected = tokens.is_some();
        Ok(Self {
            client_id,
            file,
            tokens,
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(15)))
                .http_status_as_error(false)
                .max_redirects(0)
                .build()
                .into(),
            token_url: "https://accounts.spotify.com/api/token".into(),
            base: Url::parse("https://api.spotify.com/v1/").unwrap(),
            not_before: Instant::now(),
            snapshot: Snapshot {
                authorization: if connected {
                    AuthorizationState::Connected
                } else {
                    AuthorizationState::Disconnected
                },
                ..Default::default()
            },
        })
    }
    fn save(&self) -> Result<()> {
        let parent = self.file.parent().ok_or(Error::Storage)?;
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|_| Error::Storage)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                    .map_err(|_| Error::Storage)?;
            }
        }
        let Some(tokens) = &self.tokens else {
            return match fs::remove_file(&self.file) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(Error::Storage),
            };
        };
        let temporary = parent.join(format!(".spotify-{}.tmp", random()?));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| {
            let mut f = options.open(&temporary).map_err(|_| Error::Storage)?;
            let bytes = serde_json::to_vec(tokens).map_err(|_| Error::Storage)?;
            f.write_all(&bytes).map_err(|_| Error::Storage)?;
            f.sync_all().map_err(|_| Error::Storage)?;
            fs::rename(&temporary, &self.file).map_err(|_| Error::Storage)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
    fn invalidate(&mut self) -> Error {
        self.tokens = None;
        self.snapshot.authorization = AuthorizationState::ReauthorizationRequired;
        self.snapshot.selected_device = None;
        self.snapshot.devices.clear();
        if self.save().is_err() {
            return Error::Storage;
        }
        Error::ReauthorizationRequired
    }
    pub fn begin_authorization(&mut self) -> Result<Authorization> {
        let listener =
            TcpListener::bind("127.0.0.1:43821").map_err(|_| Error::CallbackUnavailable)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| Error::CallbackUnavailable)?;
        let auth = Authorization {
            verifier: random()?,
            state: random()?,
            listener,
            started: Instant::now(),
        };
        let mut url = Url::parse("https://accounts.spotify.com/authorize").unwrap();
        url.query_pairs_mut().extend_pairs([
            ("client_id", self.client_id.as_str()),
            ("response_type", "code"),
            ("redirect_uri", REDIRECT),
            ("scope", SCOPES),
            ("state", &auth.state),
            ("code_challenge_method", "S256"),
            ("code_challenge", &challenge(&auth.verifier)),
        ]);
        self.snapshot.authorization_url = Some(url.into());
        self.snapshot.authorization = AuthorizationState::Authorizing;
        Ok(auth)
    }
    /// Called by the diagnostic worker, never the GUI thread. Bounded callback
    /// read, exact path and state, no request/response payload logging.
    pub fn finish_authorization(&mut self, auth: &Authorization) -> Result<bool> {
        if auth.started.elapsed() > Duration::from_secs(600) {
            return Err(Error::AuthorizationTimedOut);
        }
        let (mut stream, _) = match auth.listener.accept() {
            Ok(pair) => pair,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(_) => return Err(Error::CallbackUnavailable),
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|_| Error::CallbackUnavailable)?;
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|_| Error::CallbackUnavailable)?;
        let mut bytes = Vec::new();
        let mut byte = [0];
        while bytes.len() < 8192 && !bytes.ends_with(b"\r\n\r\n") {
            if stream
                .read(&mut byte)
                .map_err(|_| Error::CallbackUnavailable)?
                == 0
            {
                break;
            }
            bytes.push(byte[0]);
        }
        let request = String::from_utf8(bytes).map_err(|_| Error::InvalidState)?;
        let target = request
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("GET "))
            .and_then(|l| l.split_once(" HTTP/"))
            .map(|p| p.0)
            .ok_or(Error::InvalidState)?;
        let result = callback(target, &auth.state);
        let message = if result.is_ok() {
            "Authorization received. Return to Music Library."
        } else {
            "Authorization rejected. Return to Music Library."
        };
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
            message.len(),
            message
        );
        let code = result?;
        self.exchange(
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", REDIRECT),
                ("code_verifier", &auth.verifier),
            ],
            true,
        )?;
        self.snapshot.authorization_url = None;
        Ok(true)
    }
    fn exchange(&mut self, fields: &[(&str, &str)], initial: bool) -> Result<()> {
        self.ready()?;
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        form.append_pair("client_id", &self.client_id)
            .extend_pairs(fields.iter().copied());
        self.snapshot.token_requests += 1;
        let started = Instant::now();
        let response = self
            .agent
            .post(&self.token_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send(form.finish());
        self.snapshot.token_http_ms += started.elapsed().as_millis();
        let response = response.map_err(|_| Error::Transport)?;
        self.snapshot.last_http_status = Some(response.status().as_u16());
        let value = self.response(response)?;
        let access = value["access_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(Error::InvalidResponse)?;
        let expires = value["expires_in"]
            .as_u64()
            .filter(|s| *s > 0)
            .ok_or(Error::InvalidResponse)?;
        if !value["token_type"]
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("bearer"))
        {
            return Err(Error::InvalidResponse);
        }
        if let Some(scope) = value["scope"].as_str()
            && !SCOPES
                .split_whitespace()
                .all(|s| scope.split_whitespace().any(|v| v == s))
        {
            return Err(Error::InsufficientScope);
        }
        let refresh = value["refresh_token"]
            .as_str()
            .map(String::from)
            .or_else(|| {
                if initial {
                    None
                } else {
                    self.tokens.as_ref().map(|t| t.refresh_token.clone())
                }
            })
            .ok_or(Error::InvalidResponse)?;
        let authorized_at = if initial {
            now()
        } else {
            self.tokens
                .as_ref()
                .ok_or(Error::AuthorizationRequired)?
                .authorized_at
        };
        self.tokens = Some(Tokens {
            client_id: self.client_id.clone(),
            access_token: access.into(),
            refresh_token: refresh,
            access_expires_at: now() + expires.saturating_sub(30.min(expires / 10)),
            authorized_at,
        });
        self.save()?;
        self.snapshot.authorization = AuthorizationState::Connected;
        Ok(())
    }
    fn refresh(&mut self) -> Result<()> {
        let refresh = self
            .tokens
            .as_ref()
            .ok_or(Error::AuthorizationRequired)?
            .refresh_token
            .clone();
        let result = self.exchange(
            &[("grant_type", "refresh_token"), ("refresh_token", &refresh)],
            false,
        );
        if matches!(result, Err(Error::AuthorizationRequired)) {
            return Err(self.invalidate());
        }
        result
    }
    fn ready(&self) -> Result<()> {
        if self.not_before > Instant::now() {
            return Err(Error::RateLimited(
                self.not_before
                    .duration_since(Instant::now())
                    .as_secs()
                    .saturating_add(1),
            ));
        }
        Ok(())
    }
    fn response(&mut self, mut response: ureq::http::Response<ureq::Body>) -> Result<Value> {
        let status = response.status().as_u16();
        if status == 429 {
            let seconds = response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(30)
                .max(1);
            self.not_before = Instant::now() + Duration::from_secs(seconds.min(86400));
            return Err(Error::RateLimited(seconds));
        }
        if status >= 500 {
            self.not_before = Instant::now() + Duration::from_secs(30);
            return Err(Error::ServiceUnavailable);
        }
        if status == 204 {
            return Ok(Value::Null);
        }
        let body = response
            .body_mut()
            .with_config()
            .limit(1_000_000)
            .read_to_string()
            .map_err(|_| Error::InvalidResponse)?;
        let value: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        if value["error"].as_str() == Some("invalid_grant") {
            return Err(self.invalidate());
        }
        if matches!(
            value["error"].as_str(),
            Some("invalid_client" | "unauthorized_client")
        ) {
            return Err(Error::Configuration);
        }
        match status {
            200..=299 => Ok(value),
            401 => Err(Error::AuthorizationRequired),
            403 => {
                let reason = value["error"]["reason"].as_str().unwrap_or_default();
                let message = value["error"]["message"]
                    .as_str()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                Err(
                    if reason == "PREMIUM_REQUIRED" || message.contains("premium") {
                        Error::CapabilityUnavailable
                    } else if message.contains("scope") {
                        Error::InsufficientScope
                    } else if reason == "RESTRICTION_VIOLATED" {
                        Error::RestrictedDevice
                    } else {
                        Error::ApiRejected(status)
                    },
                )
            }
            404 => Err(Error::DeviceUnavailable),
            _ => Err(Error::ApiRejected(status)),
        }
    }
    fn request(&mut self, path: &str, body: Option<Value>) -> Result<Value> {
        self.ready()?;
        if self.snapshot.authorization == AuthorizationState::ReauthorizationRequired {
            return Err(Error::ReauthorizationRequired);
        }
        if self
            .tokens
            .as_ref()
            .ok_or(Error::AuthorizationRequired)?
            .access_expires_at
            <= now()
        {
            self.refresh()?;
        }
        for attempt in 0..2 {
            let token = &self
                .tokens
                .as_ref()
                .ok_or(Error::AuthorizationRequired)?
                .access_token;
            let url = self.base.join(path).map_err(|_| Error::Configuration)?;
            self.snapshot.api_requests += 1;
            let started = Instant::now();
            let response = if let Some(body) = &body {
                self.agent
                    .put(url.as_str())
                    .header("Authorization", &format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .send(if body.is_null() {
                        String::new()
                    } else {
                        body.to_string()
                    })
            } else {
                self.agent
                    .get(url.as_str())
                    .header("Authorization", &format!("Bearer {token}"))
                    .call()
            };
            self.snapshot.api_http_ms += started.elapsed().as_millis();
            let response = response.map_err(|_| {
                self.not_before = Instant::now() + Duration::from_secs(30);
                Error::Transport
            })?;
            self.snapshot.last_http_status = Some(response.status().as_u16());
            if response.status().as_u16() == 401 {
                if attempt == 0 {
                    self.refresh()?;
                    continue;
                }
                return Err(self.invalidate());
            }
            return self.response(response);
        }
        Err(Error::AuthorizationRequired)
    }
    pub fn devices(&mut self) -> Result<()> {
        let response = self.request("me/player/devices", None)?;
        self.snapshot.devices = serde_json::from_value(response["devices"].clone())
            .map_err(|_| Error::InvalidResponse)?;
        if self.snapshot.selected_device.as_ref().is_some_and(|id| {
            !self
                .snapshot
                .devices
                .iter()
                .any(|d| d.id.as_ref() == Some(id) && !d.is_restricted)
        }) {
            self.snapshot.selected_device = None;
        }
        if self.snapshot.devices.is_empty() {
            return Err(Error::NoDevice);
        }
        Ok(())
    }
    pub fn select_device(&mut self, id: &str) -> Result<()> {
        let device = self
            .snapshot
            .devices
            .iter()
            .find(|d| d.id.as_deref() == Some(id))
            .ok_or(Error::DeviceUnavailable)?;
        if device.is_restricted {
            return Err(Error::RestrictedDevice);
        }
        self.snapshot.selected_device = Some(id.into());
        Ok(())
    }
    pub fn poll(&mut self) -> Result<()> {
        let v = self.request("me/player", None)?;
        self.snapshot.state = State {
            track_id: v["item"]["id"].as_str().map(String::from),
            title: v["item"]["name"].as_str().unwrap_or_default().into(),
            progress_ms: v["progress_ms"].as_u64().unwrap_or(0),
            playing: v["is_playing"].as_bool().unwrap_or(false),
            device: if v["device"].is_null() {
                None
            } else {
                Some(
                    serde_json::from_value(v["device"].clone())
                        .map_err(|_| Error::InvalidResponse)?,
                )
            },
        };
        Ok(())
    }
    fn command(
        &mut self,
        operation: &str,
        body: Value,
        extra: Option<(&str, String)>,
    ) -> Result<()> {
        let id = self
            .snapshot
            .selected_device
            .clone()
            .ok_or(Error::NoDevice)?;
        self.select_device(&id)?;
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("device_id", &id);
        if let Some((key, value)) = extra {
            query.append_pair(key, &value);
        }
        let result = self
            .request(
                &format!("me/player/{operation}?{}", query.finish()),
                Some(body),
            )
            .map(|_| ());
        if result == Err(Error::DeviceUnavailable) {
            self.snapshot.selected_device = None;
            let _ = self.devices(); // Rediscover, never silently choose another device or replay command.
        }
        result
    }
    pub fn play(&mut self, song: &Song) -> Result<()> {
        // Observe immediately before deciding to resume: never trust a stale poll
        // or a remembered command when another Connect controller may intervene.
        if self.snapshot.selected_device.is_none() {
            return Err(Error::NoDevice);
        }
        self.poll()?;
        let same_device = self
            .snapshot
            .state
            .device
            .as_ref()
            .and_then(|d| d.id.as_ref())
            == self.snapshot.selected_device.as_ref();
        let resume = same_device && self.snapshot.state.track_id.as_deref() == Some(&song.0);
        self.command(
            "play",
            if resume {
                Value::Null
            } else {
                json!({"uris": [song.uri()]})
            },
            None,
        )
    }
    pub fn pause(&mut self) -> Result<()> {
        self.command("pause", Value::Null, None)
    }
    pub fn seek(&mut self, milliseconds: u64) -> Result<()> {
        self.command(
            "seek",
            Value::Null,
            Some(("position_ms", milliseconds.to_string())),
        )
    }
}

fn callback(target: &str, expected: &str) -> Result<String> {
    let url =
        Url::parse(&format!("http://127.0.0.1:43821{target}")).map_err(|_| Error::InvalidState)?;
    if url.path() != "/callback" {
        return Err(Error::InvalidState);
    }
    let pairs: Vec<_> = url.query_pairs().collect();
    let states: Vec<_> = pairs.iter().filter(|(k, _)| k == "state").collect();
    if states.len() != 1 || states[0].1 != expected {
        return Err(Error::InvalidState);
    }
    if pairs.iter().any(|(k, _)| k == "error") {
        return Err(Error::Denied);
    }
    let codes: Vec<_> = pairs.iter().filter(|(k, _)| k == "code").collect();
    if codes.len() != 1 || codes[0].1.is_empty() {
        return Err(Error::InvalidState);
    }
    Ok(codes[0].1.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        net::TcpStream,
        sync::{Arc, Mutex},
        thread,
    };

    // Requests are only retained inside tests; production never logs headers,
    // OAuth bodies, callbacks or Spotify response bodies.
    struct Server {
        url: String,
        requests: Arc<Mutex<Vec<String>>>,
        join: thread::JoinHandle<()>,
    }
    impl Server {
        fn new(responses: Vec<(u16, &'static str, &'static str)>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(vec![]));
            let saved = requests.clone();
            let join = thread::spawn(move || {
                for (status, headers, body) in responses {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut bytes = vec![];
                    let mut b = [0];
                    while !bytes.ends_with(b"\r\n\r\n") {
                        stream.read_exact(&mut b).unwrap();
                        bytes.push(b[0]);
                    }
                    let header = String::from_utf8(bytes.clone()).unwrap();
                    let length = header
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|v| v.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    let mut body_bytes = vec![0; length];
                    stream.read_exact(&mut body_bytes).unwrap();
                    bytes.extend(body_bytes);
                    saved
                        .lock()
                        .unwrap()
                        .push(String::from_utf8(bytes).unwrap());
                    write!(stream, "HTTP/1.1 {status} OK\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}", body.len()).unwrap();
                }
            });
            Self {
                url,
                requests,
                join,
            }
        }
        fn finish(self) -> Vec<String> {
            self.join.join().unwrap();
            Arc::try_unwrap(self.requests)
                .unwrap()
                .into_inner()
                .unwrap()
        }
    }
    const TOKEN: &str = r#"{"access_token":"private-access","refresh_token":"private-refresh","token_type":"Bearer","expires_in":3600}"#;
    const REFRESH: &str =
        r#"{"access_token":"renewed-access","token_type":"Bearer","expires_in":3600}"#;
    const DEVICES: &str = r#"{"devices":[{"id":"desktop","name":"Test PC","type":"Computer","is_active":true,"is_restricted":false,"supports_volume":true},{"id":"restricted","name":"Restricted","type":"Speaker","is_active":false,"is_restricted":true}]}"#;
    const STATE: &str = r#"{"item":{"id":"1234567890123456789012","name":"Song"},"progress_ms":1200,"is_playing":false,"device":{"id":"desktop","name":"Test PC","type":"Computer","is_active":true,"is_restricted":false}}"#;
    fn client(server: &Server, dir: &tempfile::TempDir) -> Playback {
        let mut p =
            Playback::new("test-client".into(), dir.path().join("credentials.json")).unwrap();
        p.base = Url::parse(&server.url).unwrap();
        p.token_url = format!("{}token", server.url);
        p
    }
    fn authorize(p: &mut Playback) {
        p.exchange(
            &[
                ("grant_type", "authorization_code"),
                ("code", "private-code"),
                ("code_verifier", "private-verifier"),
                ("redirect_uri", REDIRECT),
            ],
            true,
        )
        .unwrap();
    }
    fn song() -> Song {
        Song::from_associations(&[music_library::domain::ExternalIdentity {
            provider: "spotify".into(),
            kind: "track".into(),
            external_id: "1234567890123456789012".into(),
        }])
        .unwrap()
    }

    #[test]
    fn pkce_s256_rfc_vector_and_randomness() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let a = random().unwrap();
        let b = random().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert!(
            a.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
    }
    #[test]
    fn callback_validates_state_denial_path_and_duplicates() {
        assert_eq!(
            callback("/callback?state=expected&code=ok", "expected"),
            Ok("ok".into())
        );
        for target in [
            "/callback?state=wrong&code=ok",
            "/callback?code=ok",
            "/callback?state=expected&state=expected&code=ok",
            "/other?state=expected&code=ok",
        ] {
            assert_eq!(callback(target, "expected"), Err(Error::InvalidState));
        }
        assert_eq!(
            callback("/callback?state=expected&error=access_denied", "expected"),
            Err(Error::Denied)
        );
    }
    #[test]
    fn loopback_exchange_and_restart_do_not_persist_verifier() {
        let server = Server::new(vec![(200, "", TOKEN)]);
        let dir = tempfile::tempdir().unwrap();
        let mut p = client(&server, &dir);
        let auth = p.begin_authorization().unwrap();
        let url = Url::parse(p.snapshot.authorization_url.as_ref().unwrap()).unwrap();
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs["scope"], SCOPES);
        assert_eq!(pairs["redirect_uri"], REDIRECT);
        assert_eq!(pairs["code_challenge_method"], "S256");
        let mut stream = TcpStream::connect("127.0.0.1:43821").unwrap();
        write!(
            stream,
            "GET /callback?state={}&code=private-code HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            auth.state
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !p.finish_authorization(&auth).unwrap() {
            assert!(Instant::now() < deadline, "callback did not arrive");
            thread::sleep(Duration::from_millis(5));
        }
        let file = fs::read_to_string(&p.file).unwrap();
        assert!(!file.contains(&auth.verifier));
        assert!(!file.contains("private-code"));
        let reopened = Playback::new("test-client".into(), p.file.clone()).unwrap();
        assert_eq!(
            reopened.snapshot.authorization,
            AuthorizationState::Connected
        );
        assert!(reopened.snapshot.selected_device.is_none());
        assert!(reopened.snapshot.devices.is_empty());
        assert!(!format!("{p:?} {:?}", p.snapshot).contains("private-access"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&p.file).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let requests = server.finish();
        assert!(requests[0].contains("code_verifier="));
        assert!(!requests[0].contains("client_secret"));
        assert!(!requests[0].contains("Basic "));
    }
    #[test]
    fn refresh_preserves_refresh_token_and_original_authorization_time() {
        let s = Server::new(vec![
            (200, "", TOKEN),
            (200, "", REFRESH),
            (200, "", DEVICES),
        ]);
        let d = tempfile::tempdir().unwrap();
        let mut p = client(&s, &d);
        authorize(&mut p);
        p.tokens.as_mut().unwrap().access_expires_at = 0;
        let original = p.tokens.as_ref().unwrap().authorized_at;
        p.devices().unwrap();
        assert_eq!(p.tokens.as_ref().unwrap().refresh_token, "private-refresh");
        assert_eq!(p.tokens.as_ref().unwrap().authorized_at, original);
        assert_eq!(p.snapshot.token_requests, 2);
        assert_eq!(p.snapshot.api_requests, 1);
        let r = s.finish();
        assert!(r[1].contains("grant_type=refresh_token"));
        assert!(r[2].contains("Bearer renewed-access"));
    }
    #[test]
    fn invalid_grant_removes_persisted_session_and_stops_retries() {
        let s = Server::new(vec![
            (200, "", TOKEN),
            (400, "", r#"{"error":"invalid_grant"}"#),
        ]);
        let d = tempfile::tempdir().unwrap();
        let mut p = client(&s, &d);
        authorize(&mut p);
        p.tokens.as_mut().unwrap().access_expires_at = 0;
        assert_eq!(p.devices(), Err(Error::ReauthorizationRequired));
        assert_eq!(p.devices(), Err(Error::ReauthorizationRequired));
        assert!(!p.file.exists());
        assert_eq!(
            p.snapshot.authorization,
            AuthorizationState::ReauthorizationRequired
        );
        s.finish();
    }
    #[test]
    fn devices_explicit_selection_restrictions_and_no_implicit_restore() {
        let s = Server::new(vec![
            (200, "", TOKEN),
            (200, "", DEVICES),
            (200, "", r#"{"devices":[]}"#),
        ]);
        let d = tempfile::tempdir().unwrap();
        let mut p = client(&s, &d);
        authorize(&mut p);
        p.devices().unwrap();
        assert!(p.snapshot.devices[0].is_active);
        assert!(p.snapshot.selected_device.is_none());
        assert_eq!(p.select_device("restricted"), Err(Error::RestrictedDevice));
        assert_eq!(p.pause(), Err(Error::NoDevice));
        p.select_device("desktop").unwrap();
        assert_eq!(p.devices(), Err(Error::NoDevice));
        assert!(p.snapshot.selected_device.is_none());
        assert_eq!(p.pause(), Err(Error::NoDevice));
        s.finish();
    }
    #[test]
    fn start_resume_pause_seek_and_external_change() {
        let s = Server::new(vec![
            (200, "", TOKEN),
            (200, "", DEVICES),
            (204, "", ""),
            (204, "", ""),
            (200, "", STATE),
            (204, "", ""),
            (204, "", ""),
            (204, "", ""),
            (
                200,
                "",
                r#"{"item":{"id":"external","name":"External"},"is_playing":true,"progress_ms":9000}"#,
            ),
        ]);
        let d = tempfile::tempdir().unwrap();
        let mut p = client(&s, &d);
        authorize(&mut p);
        p.devices().unwrap();
        p.select_device("desktop").unwrap();
        p.play(&song()).unwrap();
        p.play(&song()).unwrap();
        p.pause().unwrap();
        p.seek(42000).unwrap();
        p.poll().unwrap();
        assert_eq!(p.snapshot.state.track_id.as_deref(), Some("external"));
        assert_eq!(p.snapshot.state.progress_ms, 9000);
        assert!(p.snapshot.state.playing);
        let r = s.finish();
        assert!(r[3].contains("PUT /me/player/play?device_id=desktop"));
        assert!(r[3].contains(&song().uri()));
        assert!(!r[3].contains("context_uri"));
        assert!(!r[5].contains("uris"));
        assert!(r[6].contains("/pause?device_id=desktop"));
        assert!(r[7].contains("/seek?device_id=desktop&position_ms=42000"));
        assert_eq!(r.len(), 9);
    }
    #[test]
    fn stale_device_rediscovers_without_replaying_command() {
        let s = Server::new(vec![
            (200, "", TOKEN),
            (200, "", DEVICES),
            (404, "", r#"{"error":{"message":"No active device"}}"#),
            (200, "", DEVICES),
        ]);
        let d = tempfile::tempdir().unwrap();
        let mut p = client(&s, &d);
        authorize(&mut p);
        p.devices().unwrap();
        p.select_device("desktop").unwrap();
        assert_eq!(p.pause(), Err(Error::DeviceUnavailable));
        assert!(p.snapshot.selected_device.is_none());
        assert_eq!(s.finish().len(), 4);
    }
    #[test]
    fn unauthorized_refreshes_once_then_requires_reauthorization() {
        let s = Server::new(vec![
            (200, "", TOKEN),
            (401, "", "{}"),
            (200, "", REFRESH),
            (401, "", "{}"),
        ]);
        let d = tempfile::tempdir().unwrap();
        let mut p = client(&s, &d);
        authorize(&mut p);
        assert_eq!(p.devices(), Err(Error::ReauthorizationRequired));
        assert_eq!(p.snapshot.token_requests, 2);
        assert_eq!(p.snapshot.api_requests, 2);
        assert_eq!(p.devices(), Err(Error::ReauthorizationRequired));
        s.finish();
    }

    #[test]
    fn token_endpoint_unauthorized_also_ends_the_session() {
        let s = Server::new(vec![(200, "", TOKEN), (401, "", "{}")]);
        let d = tempfile::tempdir().unwrap();
        let mut p = client(&s, &d);
        authorize(&mut p);
        p.tokens.as_mut().unwrap().access_expires_at = 0;
        assert_eq!(p.devices(), Err(Error::ReauthorizationRequired));
        assert_eq!(p.devices(), Err(Error::ReauthorizationRequired));
        assert!(!p.file.exists());
        s.finish();
    }
    #[test]
    fn rate_limits_and_service_failures_do_not_loop() {
        for (status, headers, expected) in [
            (429, "Retry-After: 60\r\n", Error::RateLimited(60)),
            (503, "", Error::ServiceUnavailable),
        ] {
            let s = Server::new(vec![(200, "", TOKEN), (status, headers, "{}")]);
            let d = tempfile::tempdir().unwrap();
            let mut p = client(&s, &d);
            authorize(&mut p);
            assert_eq!(p.devices(), Err(expected));
            assert!(matches!(p.devices(), Err(Error::RateLimited(_))));
            assert_eq!(p.snapshot.api_requests, 1);
            s.finish();
        }
    }
    #[test]
    fn capability_scope_and_transport_errors_are_playback_only() {
        for (body, expected) in [
            (
                r#"{"error":{"reason":"PREMIUM_REQUIRED"}}"#,
                Error::CapabilityUnavailable,
            ),
            (
                r#"{"error":{"message":"Insufficient scope"}}"#,
                Error::InsufficientScope,
            ),
        ] {
            let s = Server::new(vec![(200, "", TOKEN), (403, "", body)]);
            let d = tempfile::tempdir().unwrap();
            let mut p = client(&s, &d);
            authorize(&mut p);
            assert_eq!(p.devices(), Err(expected));
            assert_eq!(p.snapshot.authorization, AuthorizationState::Connected);
            s.finish();
        }
        let s = Server::new(vec![(200, "", TOKEN)]);
        let d = tempfile::tempdir().unwrap();
        let mut p = client(&s, &d);
        authorize(&mut p);
        s.finish();
        assert_eq!(p.devices(), Err(Error::Transport));
        assert!(matches!(p.devices(), Err(Error::RateLimited(_))));
    }
    #[test]
    fn no_association_and_recording_ids_cannot_construct_playback_request() {
        assert_eq!(Song::from_associations(&[]), Err(Error::NoAssociation));
        assert_eq!(song().uri(), "spotify:track:1234567890123456789012");
        for (provider, kind, value) in [
            ("spotify", "recording", "1234567890123456789012"),
            ("musicbrainz", "track", "1234567890123456789012"),
            ("spotify", "track", "bad/id"),
        ] {
            assert_eq!(
                Song::from_associations(&[music_library::domain::ExternalIdentity {
                    provider: provider.into(),
                    kind: kind.into(),
                    external_id: value.into()
                }]),
                Err(Error::NoAssociation)
            );
        }
    }

    #[test]
    fn credential_file_permissions_and_account_configuration_are_explicit() {
        let s = Server::new(vec![(200, "", TOKEN)]);
        let d = tempfile::tempdir().unwrap();
        let mut p = client(&s, &d);
        authorize(&mut p);
        assert_eq!(
            Playback::new("different-client".into(), p.file.clone())
                .unwrap()
                .snapshot
                .authorization,
            AuthorizationState::Disconnected
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p.file, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(matches!(
                Playback::new("test-client".into(), p.file.clone()),
                Err(Error::Storage)
            ));
        }
        s.finish();
        let s = Server::new(vec![(
            400,
            "",
            r#"{"error":"invalid_client","error_description":"private-secret-must-not-escape"}"#,
        )]);
        let mut p = client(&s, &tempfile::tempdir().unwrap());
        let error = p
            .exchange(&[("grant_type", "authorization_code")], true)
            .unwrap_err();
        assert_eq!(error, Error::Configuration);
        assert!(!format!("{error} {error:?} {p:?}").contains("private-secret"));
        s.finish();
    }

    #[test]
    fn transport_timeout_is_bounded() {
        let s = Server::new(vec![(200, "", TOKEN)]);
        let d = tempfile::tempdir().unwrap();
        let mut p = client(&s, &d);
        authorize(&mut p);
        s.finish();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        p.base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        p.agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_millis(50)))
            .http_status_as_error(false)
            .build()
            .into();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let server = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            let _ = wait.recv_timeout(Duration::from_secs(2));
        });
        let start = Instant::now();
        assert_eq!(p.poll(), Err(Error::Transport));
        assert!(start.elapsed() < Duration::from_secs(1));
        release.send(()).unwrap();
        server.join().unwrap();
    }
}
