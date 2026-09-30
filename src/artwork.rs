//! Provider-neutral artwork resolution. Run `resolve` on an application worker.
//! Images are reconstructible cache files; SQLite retains provenance and misses.
use crate::{Library, Result, domain::ExternalIdentity, storage::bytes_to_path};
use lofty::{file::TaggedFileExt, probe::Probe};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_EDGE: u32 = 500;
const MAX_BYTES: u64 = 20 * 1024 * 1024;
const MISS_SECONDS: i64 = 24 * 60 * 60;

pub trait Provider: Send {
    /// A local-only pass leaves unresolved entries for a separate network worker.
    fn deferred(&self) -> bool {
        false
    }

    /// Only already-associated identities are supplied. Never search or match.
    fn fetch(&mut self, identity: &ExternalIdentity) -> Option<(String, Vec<u8>)>;
}

pub struct Resolver {
    connection: Connection,
    cache: PathBuf,
}
impl Library {
    pub fn artwork_resolver(&self, cache: PathBuf) -> Result<Resolver> {
        let path = self
            .store
            .connection
            .path()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| {
                crate::storage::Error::Invalid("Artwork requires a file-backed library".into())
            })?;
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        Ok(Resolver { connection, cache })
    }
}
pub fn cache_directory() -> Option<PathBuf> {
    directories::ProjectDirs::from("org", "music-library", "music-library")
        .map(|p| p.cache_dir().join("artwork-v1"))
}
#[derive(Default)]
struct Candidates {
    local: Vec<PathBuf>,
    identities: Vec<ExternalIdentity>,
}
impl Resolver {
    /// Batch association lookup avoids per-tile metadata queries. `track:` keys
    /// are accepted for the player; all other keys are opaque application Album IDs.
    pub fn resolve(
        &mut self,
        keys: &[String],
        provider: &mut dyn Provider,
    ) -> Result<Vec<(String, Option<PathBuf>)>> {
        self.resolve_with(keys, provider, &mut |_, _| {})
    }
    pub fn resolve_with(
        &mut self,
        keys: &[String],
        provider: &mut dyn Provider,
        ready: &mut dyn FnMut(String, Option<PathBuf>),
    ) -> Result<Vec<(String, Option<PathBuf>)>> {
        let json = serde_json::to_string(keys).expect("string list");
        let pairs: Vec<(String, String)> = self.connection.prepare(
            "SELECT j.value, CASE WHEN substr(j.value,1,6)='track:' THEN
            (SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id=substr(j.value,7))
            ELSE j.value END FROM json_each(?1) j")?.query_map([json], |r| Ok((r.get(0)?, r.get::<_,Option<String>>(1)?.unwrap_or_default())))?
            .collect::<rusqlite::Result<_>>()?;
        let albums: Vec<_> = pairs.iter().map(|(_, a)| a.clone()).collect();
        let json = serde_json::to_string(&albums).expect("string list");
        let mut candidates: HashMap<String, Candidates> = HashMap::new();
        {
            let mut stmt = self.connection.prepare("SELECT DISTINCT r.album_id,l.path FROM json_each(?1) j JOIN release r ON r.album_id=j.value JOIN track t ON t.release_id=r.id JOIN library_membership lm ON lm.track_id=t.id JOIN track_source ts ON ts.track_id=t.id JOIN local_file_observation l ON l.source_id=ts.source_id WHERE l.available=1 ORDER BY r.album_id,l.path")?;
            for row in stmt.query_map([&json], |r| {
                Ok((r.get::<_, String>(0)?, bytes_to_path(r.get(1)?)))
            })? {
                let (album, path) = row?;
                candidates.entry(album).or_default().local.push(path);
            }
            let mut stmt = self.connection.prepare("SELECT album_id,provider,kind,external_id FROM album_external_identity WHERE album_id IN (SELECT value FROM json_each(?1)) UNION SELECT r.album_id,e.provider,e.kind,e.external_id FROM release_external_identity e JOIN release r ON r.id=e.release_id WHERE r.album_id IN (SELECT value FROM json_each(?1)) ORDER BY 1,2,3,4")?;
            for row in stmt.query_map([&json], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    ExternalIdentity {
                        provider: r.get(1)?,
                        kind: r.get(2)?,
                        external_id: r.get(3)?,
                    },
                ))
            })? {
                let (album, id) = row?;
                candidates.entry(album).or_default().identities.push(id);
            }
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let cached: HashMap<String,(String,i64)> = self.connection.prepare("SELECT album_id,cache_name,checked_at FROM album_artwork WHERE album_id IN (SELECT value FROM json_each(?1))")?
            .query_map([&json], |r| Ok((r.get(0)?,(r.get(1)?,r.get(2)?))))?.collect::<rusqlite::Result<_>>()?;
        let mut resolved = HashMap::new();
        let mut albums = albums;
        albums.sort_by_key(|a| !cached.contains_key(a));
        let mut emit = |album: &str, path: Option<PathBuf>| {
            for (key, a) in &pairs {
                if a == album {
                    ready(key.clone(), path.clone());
                }
            }
        };
        for album in &albums {
            if album.is_empty() || resolved.contains_key(album) {
                continue;
            }
            if let Some((name, checked)) = cached.get(album) {
                if !name.is_empty() && *checked > 0 && valid_cached_image(&self.cache.join(name)) {
                    emit(album, Some(self.cache.join(name)));
                    resolved.insert(album.clone(), Some(self.cache.join(name)));
                    continue;
                }
                if name.is_empty() && *checked > 0 && now - checked < MISS_SECONDS {
                    emit(album, None);
                    resolved.insert(album.clone(), None);
                    continue;
                }
            }
            let c = candidates.remove(album).unwrap_or_default();
            let mut found = None;
            // All embedded candidates precede all sidecars, including malformed art.
            'embedded: for path in &c.local {
                if let Ok(file) = Probe::open(path).and_then(|p| p.read()) {
                    for tag in file.tags() {
                        let mut pictures: Vec<_> = tag.pictures().iter().collect();
                        pictures.sort_by_key(|p| {
                            p.pic_type() != lofty::picture::PictureType::CoverFront
                        });
                        for picture in pictures {
                            if let Some(image) = self.save(picture.data()) {
                                found = Some((
                                    "embedded".to_string(),
                                    path.to_string_lossy().into_owned(),
                                    image,
                                ));
                                break 'embedded;
                            }
                        }
                    }
                }
            }
            if found.is_none() {
                let mut dirs = std::collections::BTreeSet::new();
                for path in &c.local {
                    if let Some(dir) = path.parent() {
                        dirs.insert(dir);
                    }
                }
                'sidecars: for dir in dirs {
                    let mut files: Vec<_> = std::fs::read_dir(dir)
                        .into_iter()
                        .flatten()
                        .filter_map(|r| r.ok())
                        .map(|e| e.path())
                        .filter(|p| {
                            let stem = p
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("")
                                .to_ascii_lowercase();
                            let ext = p
                                .extension()
                                .and_then(|s| s.to_str())
                                .unwrap_or("")
                                .to_ascii_lowercase();
                            matches!(stem.as_str(), "cover" | "folder" | "front" | "album")
                                && matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp")
                        })
                        .collect();
                    files.sort();
                    for path in files {
                        if let Some(bytes) = read_bounded(&path)
                            && let Some(image) = self.save(&bytes)
                        {
                            found = Some((
                                "sidecar".into(),
                                path.to_string_lossy().into_owned(),
                                image,
                            ));
                            break 'sidecars;
                        }
                    }
                }
            }
            if found.is_none() && provider.deferred() {
                emit(album, None);
                resolved.insert(album.clone(), None);
                continue;
            }
            if found.is_none() {
                for provider_name in ["musicbrainz", "spotify"] {
                    for identity in c.identities.iter().filter(|i| i.provider == provider_name) {
                        if let Some((url, bytes)) = provider.fetch(identity)
                            && let Some(image) = self.save(&bytes)
                        {
                            found = Some((provider_name.into(), url, image));
                            break;
                        }
                    }
                    if found.is_some() {
                        break;
                    }
                }
            }
            let (origin, locator, name) = found.clone().unwrap_or_default();
            // Compare the observation read before I/O. A delayed download cannot
            // overwrite a newer local result or a source invalidation.
            let previous = cached.get(album).cloned().unwrap_or_default();
            let rank = |column: &str| {
                format!(
                    "CASE {column} WHEN 'embedded' THEN 0 WHEN 'sidecar' THEN 1 WHEN 'musicbrainz' THEN 2 WHEN 'spotify' THEN 3 ELSE 4 END"
                )
            };
            let sql = format!("INSERT INTO album_artwork SELECT ?1,?2,?3,?4,?5 WHERE EXISTS(SELECT 1 FROM album WHERE id=?1)
                ON CONFLICT(album_id) DO UPDATE SET
                origin=CASE WHEN excluded.origin='' THEN album_artwork.origin ELSE excluded.origin END,
                locator=CASE WHEN excluded.locator='' THEN album_artwork.locator ELSE excluded.locator END,
                cache_name=excluded.cache_name,checked_at=excluded.checked_at
                WHERE (album_artwork.cache_name=?6 AND album_artwork.checked_at=?7) OR {} < {}", rank("excluded.origin"), rank("album_artwork.origin"));
            let written = self.connection.execute(
                &sql,
                params![album, origin, locator, name, now, previous.0, previous.1],
            )?;
            if written == 0 {
                if !name.is_empty() {
                    let _ = std::fs::remove_file(self.cache.join(&name));
                }
                let latest: Option<String> = self
                    .connection
                    .query_row(
                        "SELECT cache_name FROM album_artwork WHERE album_id=?1",
                        [album],
                        |r| r.get(0),
                    )
                    .optional()?;
                let path = latest
                    .filter(|n| !n.is_empty())
                    .map(|n| self.cache.join(n))
                    .filter(|p| p.is_file());
                emit(album, path.clone());
                resolved.insert(album.clone(), path);
                continue;
            }
            if let Some((old, _)) = cached.get(album)
                && !old.is_empty()
                && *old != name
            {
                let _ = std::fs::remove_file(self.cache.join(old));
            }
            let path = found.map(|(_, _, name)| self.cache.join(name));
            emit(album, path.clone());
            resolved.insert(album.clone(), path);
        }
        Ok(pairs
            .into_iter()
            .map(|(key, album)| (key, resolved.get(&album).cloned().flatten()))
            .collect())
    }
    fn save(&self, bytes: &[u8]) -> Option<String> {
        if bytes.len() as u64 > MAX_BYTES {
            return None;
        }
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .ok()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16000);
        limits.max_image_height = Some(16000);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().ok()?;
        let image = if image.width() > MAX_EDGE || image.height() > MAX_EDGE {
            image.thumbnail(MAX_EDGE, MAX_EDGE)
        } else {
            image
        };
        std::fs::create_dir_all(&self.cache).ok()?;
        let name = format!("{}.png", uuid::Uuid::new_v4());
        image.save(self.cache.join(&name)).ok()?;
        Some(name)
    }
}
fn read_bounded(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_BYTES).then_some(bytes)
}

fn valid_cached_image(path: &Path) -> bool {
    let Ok(mut reader) = image::ImageReader::open(path) else {
        return false;
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits.max_alloc = Some(4 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().is_ok()
}
