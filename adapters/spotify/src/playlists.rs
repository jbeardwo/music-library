//! Read-only user-authorized snapshots; Spotify JSON never crosses this boundary.
use crate::playback::{Error as AuthError, Playback};
use music_library::{
    catalog::{Credit, Duration},
    domain::ExternalIdentity,
    playlist_import::{Item, Plan},
};
use serde_json::Value;
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    MalformedLink,
    Authorization(AuthError),
    AccessDenied,
    NotFound,
    InvalidResponse,
    Changed,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Self::MalformedLink => "Enter a valid Spotify playlist URL, URI or ID.".into(),
            Self::Authorization(
                AuthError::InsufficientScope
                | AuthError::AuthorizationRequired
                | AuthError::ReauthorizationRequired,
            ) => "Reconnect Spotify to authorize playlist access, then retry the import.".into(),
            Self::Authorization(AuthError::Transport) => {
                "Could not reach Spotify. Check your connection and retry.".into()
            }
            Self::Authorization(AuthError::RateLimited(s)) => {
                format!("Spotify rate limit reached. Retry after {s} seconds.")
            }
            Self::Authorization(e) => e.to_string(),
            Self::AccessDenied => {
                "Spotify did not allow this playlist to be imported with the connected account.\n\nIf this is a public playlist, try following it in Spotify and importing it again."
                    .into()
            }
            Self::NotFound => "This Spotify playlist was not found.".into(),
            Self::InvalidResponse => {
                "Spotify returned an incomplete or invalid playlist. Nothing was imported.".into()
            }
            Self::Changed => "This Spotify playlist changed during import. Please retry.".into(),
        })
    }
}
impl std::error::Error for Error {}
impl From<AuthError> for Error {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::ApiRejected(403)
            | AuthError::RestrictedDevice
            | AuthError::CapabilityUnavailable => Self::AccessDenied,
            AuthError::DeviceUnavailable | AuthError::ApiRejected(404) => Self::NotFound,
            _ => Self::Authorization(e),
        }
    }
}
type Result<T> = std::result::Result<T, Error>;
fn valid_id(id: &str) -> bool {
    id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}
pub fn parse_playlist(input: &str) -> Result<String> {
    let input = input.trim();
    if valid_id(input) {
        return Ok(input.into());
    }
    if let Some(id) = input.strip_prefix("spotify:playlist:") {
        return if valid_id(id) {
            Ok(id.into())
        } else {
            Err(Error::MalformedLink)
        };
    }
    let url = Url::parse(input).map_err(|_| Error::MalformedLink)?;
    if url.scheme() != "https"
        || url.host_str() != Some("open.spotify.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(Error::MalformedLink);
    }
    let parts: Vec<_> = url
        .path()
        .trim_end_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let parts = if parts.first().is_some_and(|p| p.starts_with("intl-")) {
        &parts[1..]
    } else {
        &parts[..]
    };
    match parts {
        ["playlist", id] if valid_id(id) => Ok((*id).into()),
        _ => Err(Error::MalformedLink),
    }
}
#[derive(Clone, Debug)]
pub struct Summary {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub count: Option<u64>,
}
fn summary(v: &Value) -> Result<Summary> {
    let id = v["id"]
        .as_str()
        .filter(|s| valid_id(s))
        .ok_or(Error::InvalidResponse)?;
    Ok(Summary {
        id: id.into(),
        name: v["name"].as_str().ok_or(Error::InvalidResponse)?.into(),
        owner: v["owner"]["display_name"]
            .as_str()
            .or(v["owner"]["id"].as_str())
            .unwrap_or_default()
            .into(),
        count: v["items"]["total"]
            .as_u64()
            .or(v["tracks"]["total"].as_u64()),
    })
}
fn credits(v: &Value) -> Vec<Credit> {
    let Some(artists) = v.as_array() else {
        return vec![];
    };
    artists
        .iter()
        .enumerate()
        .map(|(n, a)| Credit {
            name: a["name"].as_str().unwrap_or("Unknown artist").into(),
            identity: a["id"]
                .as_str()
                .filter(|s| valid_id(s))
                .map(|s| identity("artist", s)),
            join_phrase: if n + 1 < artists.len() {
                " · ".into()
            } else {
                String::new()
            },
        })
        .collect()
}
fn identity(kind: &str, id: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "spotify".into(),
        kind: kind.into(),
        external_id: id.into(),
    }
}
fn item(v: &Value) -> Result<Item> {
    let id = v["id"]
        .as_str()
        .filter(|s| valid_id(s))
        .ok_or(Error::InvalidResponse)?;
    let album = &v["album"];
    let album_id = album["id"]
        .as_str()
        .filter(|s| valid_id(s))
        .ok_or(Error::InvalidResponse)?;
    Ok(Item {
        identity: identity("track", id),
        title: v["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(Error::InvalidResponse)?
            .into(),
        credits: credits(&v["artists"]),
        release_identity: identity("album", album_id),
        release_title: album["name"].as_str().unwrap_or("Unknown album").into(),
        release_credits: credits(&album["artists"]),
        year: album["release_date"]
            .as_str()
            .and_then(|d| d.get(..4))
            .and_then(|s| s.parse().ok()),
        disc: v["disc_number"].as_u64().and_then(|v| v.try_into().ok()),
        number: v["track_number"].as_u64().and_then(|v| v.try_into().ok()),
        duration: v["duration_ms"].as_u64().map(|milliseconds| Duration {
            milliseconds,
            approximate: false,
        }),
    })
}
/// Validate pagination URLs without sending user tokens to arbitrary response-provided hosts.
fn next_offset(v: &Value, path: &str, offset: u64) -> Result<Option<u64>> {
    match &v["next"] {
        Value::Null => Ok(None),
        Value::String(next) => {
            let url = Url::parse(next).map_err(|_| Error::InvalidResponse)?;
            if url.origin() != Url::parse("https://api.spotify.com").unwrap().origin()
                || url.path() != format!("/v1/{path}")
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(Error::InvalidResponse);
            }
            let next = url
                .query_pairs()
                .find(|(k, _)| k == "offset")
                .and_then(|(_, v)| v.parse::<u64>().ok())
                .filter(|n| *n > offset)
                .ok_or(Error::InvalidResponse)?;
            Ok(Some(next))
        }
        _ => Err(Error::InvalidResponse),
    }
}
impl Playback {
    fn playlist_request(&mut self, path: &str) -> Result<Value> {
        // Missing saved scopes are checked before any API request. A refused
        // playlist read with a scoped grant is an access error, not reconnect.
        self.request(path, None).map_err(|error| match error {
            AuthError::InsufficientScope => Error::AccessDenied,
            other => other.into(),
        })
    }
    pub fn account_playlists(&mut self) -> Result<Vec<Summary>> {
        self.require_playlist_scopes()?;
        let mut playlists = Vec::new();
        let mut offset = 0;
        loop {
            let page = self.playlist_request(&format!("me/playlists?limit=50&offset={offset}"))?;
            let values = page["items"].as_array().ok_or(Error::InvalidResponse)?;
            for value in values {
                playlists.push(summary(value)?);
            }
            match next_offset(&page, "me/playlists", offset)? {
                // Account pages may omit inaccessible/deleted playlists while
                // total and offsets still include them. Follow every next page;
                // do not equate returned visible rows with the remote total.
                Some(next) => offset = next,
                None => break,
            }
        }
        Ok(playlists)
    }
    pub fn fetch_playlist(&mut self, input: &str, progress: &mut dyn FnMut(usize)) -> Result<Plan> {
        let id = parse_playlist(input)?;
        self.require_playlist_scopes()?;
        let metadata = self.playlist_request(&format!("playlists/{id}"))?;
        let info = summary(&metadata)?;
        if info.id != id {
            return Err(Error::InvalidResponse);
        }
        let mut plan = Plan {
            provider: "spotify".into(),
            external_id: id.clone(),
            source_url: format!("https://open.spotify.com/playlist/{id}"),
            version: metadata["snapshot_id"].as_str().map(Into::into),
            owner: info.owner,
            name: info.name,
            items: vec![],
            unsupported: 0,
            unavailable: 0,
        };
        let path = format!("playlists/{id}/items");
        let mut offset = 0;
        let mut seen = 0;
        loop {
            let page = self.playlist_request(&format!(
                "{path}?limit=50&offset={offset}&additional_types=track,episode"
            ))?;
            let values = page["items"].as_array().ok_or(Error::InvalidResponse)?;
            for value in values {
                // Current API uses item; tolerate older response shapes, never the old endpoint.
                let track = value
                    .get("item")
                    .or_else(|| value.get("track"))
                    .unwrap_or(&Value::Null);
                if track.is_null() {
                    plan.unavailable += 1;
                } else if track["type"].as_str() != Some("track")
                    || value["is_local"].as_bool() == Some(true)
                    || track["is_local"].as_bool() == Some(true)
                {
                    plan.unsupported += 1;
                } else if track["is_playable"].as_bool() == Some(false)
                    || track
                        .get("restrictions")
                        .is_some_and(|v| v["reason"].is_string())
                {
                    plan.unavailable += 1;
                } else {
                    match item(track) {
                        Ok(t) => plan.items.push(t),
                        Err(_) => plan.unavailable += 1,
                    }
                }
            }
            seen += values.len();
            progress(seen);
            match next_offset(&page, &path, offset)? {
                Some(next) if next == seen as u64 && !values.is_empty() => offset = next,
                Some(_) => return Err(Error::InvalidResponse),
                None => {
                    if page["total"].as_u64() != Some(seen as u64) {
                        return Err(Error::InvalidResponse);
                    }
                    break;
                }
            }
        }
        // Prevent a mixed snapshot if edits occurred while pagination was in flight.
        let final_metadata =
            self.playlist_request(&format!("playlists/{id}?fields=snapshot_id"))?;
        if final_metadata["snapshot_id"].as_str() != plan.version.as_deref() {
            return Err(Error::Changed);
        }
        Ok(plan)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn urls_and_uris() {
        let id = "3cEYpjA9oz9GiPac4AsH4n";
        for input in [
            id.to_owned(),
            format!("spotify:playlist:{id}"),
            format!("https://open.spotify.com/playlist/{id}?si=abc"),
            format!("https://open.spotify.com/intl-de/playlist/{id}/"),
        ] {
            assert_eq!(parse_playlist(&input).unwrap(), id);
        }
        for input in [
            "https://evil.test/playlist/3cEYpjA9oz9GiPac4AsH4n",
            "https://open.spotify.com.evil.test/playlist/3cEYpjA9oz9GiPac4AsH4n",
            "spotify:track:3cEYpjA9oz9GiPac4AsH4n",
            "https://open.spotify.com/playlist/%2f..",
            "bad",
            "https://user@open.spotify.com/playlist/3cEYpjA9oz9GiPac4AsH4n",
        ] {
            assert_eq!(parse_playlist(input), Err(Error::MalformedLink));
        }
    }
    #[test]
    fn pagination_url_boundary() {
        assert_eq!(
            next_offset(
                &serde_json::json!({"next":"https://evil.test/v1/me/playlists?offset=50"}),
                "me/playlists",
                0
            ),
            Err(Error::InvalidResponse)
        );
        assert_eq!(
            next_offset(
                &serde_json::json!({"next":"https://api.spotify.com/v1/me/playlists?offset=50"}),
                "me/playlists",
                0
            )
            .unwrap(),
            Some(50)
        );
    }
}
