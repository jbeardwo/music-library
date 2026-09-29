//! Interactive catalog workers. One active and one replaceable pending request per provider.
use crate::{Bridge, string};
use music_library::{
    catalog::*,
    catalog_search::{Context, Hit, Kind, rank},
};
use qmetaobject::{QPointer, QVariant, QVariantList, QVariantMap};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone)]
enum Request {
    Search(String, Kind),
    Artist(ArtistCandidate, u32),
    Detail(Box<Hit>, Option<music_library::domain::ExternalIdentity>),
}
enum Reply {
    Hits(Vec<Hit>),
    Artist(Box<(ArtistCandidate, Page<AlbumCandidate>)>),
    Detail(Box<(Release, Option<SongCandidate>)>),
    Error(String),
    Done,
}
type Queue = Arc<(Mutex<(bool, Option<(u64, Request)>)>, Condvar)>;
struct Worker {
    provider: String,
    queue: Queue,
}
impl Drop for Worker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.queue;
        let mut q = lock.lock().unwrap();
        q.0 = true;
        q.1 = None;
        wake.notify_one();
    }
}
impl Worker {
    fn new(
        provider: String,
        mut client: Box<dyn CatalogProvider>,
        generation: Arc<AtomicU64>,
        emit: impl Fn(u64, Reply) + Send + 'static,
    ) -> Self {
        let queue: Queue = Arc::new((Mutex::new((false, None)), Condvar::new()));
        let inbox = queue.clone();
        std::thread::spawn(move || {
            loop {
                let request = {
                    let (lock, wake) = &*inbox;
                    let mut q = lock.lock().unwrap();
                    while !q.0 && q.1.is_none() {
                        q = wake.wait(q).unwrap();
                    }
                    if q.0 {
                        return;
                    }
                    q.1.take().unwrap()
                };
                let (g, request) = request;
                let current = || generation.load(Ordering::Relaxed) == g;
                if !current() {
                    continue;
                }
                match request {
                    Request::Search(query, kind) => {
                        for section in [Kind::Artist, Kind::Album, Kind::Song] {
                            if !current() {
                                break;
                            }
                            if kind != Kind::All && kind != section {
                                continue;
                            }
                            let result = match section {
                                Kind::Artist => client.search_artists(&query).map(|p| {
                                    p.items.into_iter().take(20).map(Hit::Artist).collect()
                                }),
                                Kind::Album => client.catalog_albums(&query).map(|p| {
                                    p.items.into_iter().take(20).map(Hit::Album).collect()
                                }),
                                _ => client.catalog_songs(&query).map(|p| {
                                    p.items
                                        .into_iter()
                                        .take(20)
                                        .map(|s| Hit::Song(Box::new(s)))
                                        .collect()
                                }),
                            };
                            let unavailable = result.as_ref().err().is_some_and(|e| {
                                e.is_provider_unavailable()
                                    || matches!(e, CatalogError::Configuration { .. })
                            });
                            if current() {
                                let reply = match result {
                                    Ok(hits) => Reply::Hits(hits),
                                    Err(_) => Reply::Error("Some catalog results are unavailable. You can keep searching or retry.".into()),
                                };
                                emit(g, reply);
                            }
                            if unavailable {
                                break;
                            }
                        }
                    }
                    Request::Artist(artist, offset) => {
                        let result = client.browse_artist(&artist.identity, offset);
                        if current() {
                            emit(
                                g,
                                match result {
                                    Ok(page) => Reply::Artist(Box::new((artist, page))),
                                    Err(_) => Reply::Error(
                                        "Could not load this Artist’s Albums. Try again.".into(),
                                    ),
                                },
                            );
                        }
                    }
                    Request::Detail(hit, preferred) => {
                        let (album, song) = match *hit {
                            Hit::Album(a) => (a, None),
                            Hit::Song(s) => (s.album.clone(), Some(*s)),
                            _ => unreachable!(),
                        };
                        let result = if let Some(id) = preferred
                            .as_ref()
                            .or_else(|| song.as_ref().and_then(|s| s.release.as_ref()))
                        {
                            client.release(id).and_then(|r| {
                                if r.album.identity == album.identity {
                                    Ok(r)
                                } else {
                                    Err(CatalogError::Other("Album context changed".into()))
                                }
                            })
                        } else {
                            client.catalog_album(&album)
                        };
                        if current() {
                            let reply = match result {
                                Ok(release) if release.media.iter().map(|m| m.tracks.len()).sum::<usize>() <= 1000 => Reply::Detail(Box::new((release, song))),
                                _ => Reply::Error("Could not load a complete, bounded track list. Go back and try another result.".into()),
                            };
                            emit(g, reply);
                        }
                    }
                }
                if current() {
                    emit(g, Reply::Done);
                }
            }
        });
        Self { provider, queue }
    }
    fn send(&self, g: u64, request: Request) {
        let (lock, wake) = &*self.queue;
        lock.lock().unwrap().1 = Some((g, request));
        wake.notify_one();
    }
}
#[derive(Clone, Default)]
struct PageState {
    hits: Vec<Hit>,
    artist: Option<ArtistCandidate>,
    next: Option<u32>,
    detail: Option<Release>,
    song: Option<SongCandidate>,
}
#[derive(Default)]
pub struct State {
    workers: Vec<Worker>,
    generation: Arc<AtomicU64>,
    pending: usize,
    query: String,
    page: PageState,
    history: Vec<PageState>,
    contexts: Vec<Context>,
    saved: Vec<(u32, u32)>,
    status: String,
    programs: std::collections::VecDeque<(music_library::domain::ExternalIdentity, usize)>,
}
impl Bridge {
    fn music_invalidate(&mut self) -> u64 {
        self.add_music.pending = 0;
        self.add_music.generation.fetch_add(1, Ordering::Relaxed) + 1
    }
    fn music_workers(&mut self) {
        if !self.add_music.workers.is_empty() {
            return;
        }
        // Each provider keeps its own auth/circuit state. MusicBrainz's process gate
        // remains shared with matching. Independent threads allow progressive results.
        let mut clients: Vec<(String, Box<dyn CatalogProvider>)> = vec![];
        let names = if self.spotify {
            vec!["spotify".to_string(), "musicbrainz".to_string()]
        } else {
            self.catalog_providers.clone()
        };
        for name in names {
            match name.as_str() {
                "spotify" => {
                    if let Ok(client) = music_library_spotify::Spotify::from_env() {
                        clients.push((name, Box::new(client)));
                    }
                }
                "musicbrainz" => clients.push((
                    name,
                    Box::new(music_library_musicbrainz::MusicBrainz::new()),
                )),
                _ => {}
            }
        }
        for (name, client) in clients {
            let pointer = QPointer::from(&*self);
            let callback = qmetaobject::queued_callback(move |(g, reply)| {
                if let Some(b) = pointer.as_pinned() {
                    let mut b = b.borrow_mut();
                    b.music_reply(g, reply);
                }
            });
            self.add_music.workers.push(Worker::new(
                name,
                client,
                self.add_music.generation.clone(),
                move |g, r| callback((g, r)),
            ));
        }
    }
    fn music_send(&mut self, request: Request, provider: Option<String>) {
        self.music_workers();
        let g = self.music_invalidate();
        self.add_music.status.clear();
        for worker in &self.add_music.workers {
            if provider.as_ref().is_none_or(|p| *p == worker.provider) {
                worker.send(g, request.clone());
                self.add_music.pending += 1;
            }
        }
        if self.add_music.pending == 0 {
            self.add_music.status = "This catalog is unavailable. Try another result.".into();
        }
    }
    pub fn music_action(&mut self, action: String, value: String) {
        if let Err(e) = self.music_action_inner(&action, &value) {
            self.add_music.status = e;
        }
        self.music_changed();
    }
    fn music_action_inner(&mut self, action: &str, value: &str) -> Result<(), String> {
        match action {
            "invalidate" | "close" => {
                self.music_invalidate();
            }
            "search" => {
                let (filter, query) = value.split_once(':').unwrap_or(("0", value));
                self.add_music.query = query.trim().chars().take(256).collect();
                self.add_music.page = PageState::default();
                self.add_music.history.clear();
                self.add_music.contexts.clear();
                self.add_music.status.clear();
                self.music_invalidate();
                if !self.add_music.query.is_empty() {
                    let kind = match filter {
                        "1" => Kind::Artist,
                        "2" => Kind::Album,
                        "3" => Kind::Song,
                        _ => Kind::All,
                    };
                    self.music_send(Request::Search(self.add_music.query.clone(), kind), None);
                }
            }
            "open" => {
                let i = value.parse::<usize>().map_err(|e| e.to_string())?;
                let hit = self
                    .add_music
                    .page
                    .hits
                    .get(i)
                    .ok_or("Search again to select a result")?
                    .clone();
                let provider = hit.identity().provider.clone();
                self.add_music.history.push(self.add_music.page.clone());
                self.add_music.page = PageState::default();
                self.add_music.contexts.clear();
                self.add_music.saved.clear();
                match hit {
                    Hit::Artist(a) => self.music_send(Request::Artist(a, 0), Some(provider)),
                    h => {
                        let album = match &h {
                            Hit::Album(a) => a,
                            Hit::Song(s) => &s.album,
                            _ => unreachable!(),
                        };
                        let reference = self
                            .session
                            .library
                            .catalog_existing_reference(album)
                            .map_err(|e| e.to_string())?;
                        self.music_send(Request::Detail(Box::new(h), reference), Some(provider));
                    }
                }
            }
            "back" => {
                self.music_invalidate();
                if let Some(page) = self.add_music.history.pop() {
                    self.add_music.page = page;
                }
                self.add_music.status.clear();
                self.music_context()?;
            }
            "more" => {
                let a = self
                    .add_music
                    .page
                    .artist
                    .clone()
                    .ok_or("Select an Artist")?;
                let offset = self.add_music.page.next.ok_or("No more Albums")?;
                self.music_send(
                    Request::Artist(a.clone(), offset),
                    Some(a.identity.provider),
                );
            }
            "album" | "song" => {
                let release = self
                    .add_music
                    .page
                    .detail
                    .clone()
                    .ok_or("Select an Album or Song")?;
                // Read membership again at the action boundary, so Back or another
                // local add cannot cause already-saved occurrences to be saved twice.
                let saved = self
                    .session
                    .library
                    .catalog_saved_positions(&release)
                    .map_err(|e| e.to_string())?;
                let imported = if action == "album" {
                    let missing: Vec<_> = release
                        .media
                        .iter()
                        .flat_map(|m| m.tracks.iter().map(move |t| (m.position, t.position)))
                        .filter(|p| !saved.contains(p))
                        .collect();
                    if missing.is_empty() {
                        self.music_context()?;
                        return Ok(());
                    }
                    self.session
                        .library
                        .add_catalog_selection(&release, &missing)
                } else {
                    let (disc, track) = value.split_once(':').ok_or("Select a Song")?;
                    let position = (
                        disc.parse::<u32>().map_err(|e| e.to_string())?,
                        track.parse::<u32>().map_err(|e| e.to_string())?,
                    );
                    if !release.media.iter().any(|m| {
                        m.position == position.0
                            && m.tracks.iter().any(|t| t.position == position.1)
                    }) {
                        return Err("Select a Song from this Album".into());
                    }
                    if saved.contains(&position) {
                        self.music_context()?;
                        return Ok(());
                    }
                    self.session
                        .library
                        .add_catalog_selection(&release, &[position])
                }
                .map_err(|e| {
                    eprintln!("catalog add failed: {e}");
                    "Could not add this selection safely. Try another Album or Song result."
                        .to_string()
                })?;
                self.enrich_catalog_spotify(&imported);
                self.browse_action_impl("refresh", 0, String::new());
                self.music_context()?;
                self.add_music.status = if action == "album" {
                    "Album added to your library."
                } else {
                    "Song added to your library."
                }
                .into();
                self.changed();
            }
            _ => return Err("Unknown Add Music action".into()),
        }
        Ok(())
    }
    fn music_context(&mut self) -> Result<(), String> {
        let contexts = self
            .session
            .library
            .catalog_context(&self.add_music.page.hits)
            .map_err(|e| e.to_string())?;
        // Never merge by spelling. Song occurrences additionally retain Album context.
        let mut seen = std::collections::HashSet::new();
        let mut hits = Vec::new();
        let mut kept = Vec::new();
        for (hit, context) in self.add_music.page.hits.drain(..).zip(contexts) {
            let album = match (&hit, &context.key) {
                (Hit::Song(s), None) => format!("{:?}", s.album.identity),
                _ => String::new(),
            };
            let key = context
                .key
                .clone()
                .unwrap_or_else(|| format!("{:?}:{:?}", hit.kind(), hit.identity()));
            if seen.insert((key, album)) {
                hits.push(hit);
                kept.push(context);
            }
        }
        self.add_music.page.hits = hits;
        self.add_music.contexts = kept;
        self.add_music.saved = match &self.add_music.page.detail {
            Some(r) => self
                .session
                .library
                .catalog_saved_positions(r)
                .map_err(|e| e.to_string())?,
            None => vec![],
        };
        Ok(())
    }
    fn music_reply(&mut self, g: u64, reply: Reply) {
        if g != self.add_music.generation.load(Ordering::Relaxed) {
            return;
        }
        match reply {
            Reply::Hits(hits) => {
                self.add_music.page.hits.extend(hits);
                rank(&mut self.add_music.page.hits, &self.add_music.query);
            }
            Reply::Artist(reply) => {
                let (artist, page) = *reply;
                self.add_music.page.artist = Some(artist);
                self.add_music.page.next = page.next_offset;
                self.add_music.page.hits = page.items.into_iter().map(Hit::Album).collect();
            }
            Reply::Detail(reply) => {
                let (release, song) = *reply;
                self.add_music
                    .programs
                    .retain(|(id, _)| *id != release.album.identity);
                if self.add_music.programs.len() == 8 {
                    self.add_music.programs.pop_front();
                }
                self.add_music.programs.push_back((
                    release.album.identity.clone(),
                    release.media.iter().map(|m| m.tracks.len()).sum(),
                ));
                self.add_music.page.detail = Some(release);
                self.add_music.page.song = song;
            }
            Reply::Error(e) => self.add_music.status = e,
            Reply::Done => self.add_music.pending = self.add_music.pending.saturating_sub(1),
        }
        if let Err(e) = self.music_context() {
            self.add_music.status = e;
        }
        self.music_changed();
    }
    pub fn music_value(&self) -> QVariantMap {
        let state = &self.add_music;
        let hits: QVariantList = state
            .page
            .hits
            .iter()
            .enumerate()
            .map(|(i, h)| -> QVariant {
                let (section, context) = match h {
                    Hit::Artist(a) => (
                        "ARTISTS",
                        if a.comment.is_empty() {
                            a.country.clone()
                        } else {
                            a.comment.clone()
                        },
                    ),
                    Hit::Album(a) => ("ALBUMS", a.artist.clone()),
                    Hit::Song(s) => ("SONGS", format!("{} — {}", s.artist, s.album.title)),
                };
                let saved = state.contexts.get(i).map_or(0, |c| c.saved);
                let membership = if saved == 0 {
                    String::new()
                } else if h.kind() == Kind::Song {
                    "In library".into()
                } else if let Some((_, total)) =
                    state.programs.iter().find(|(id, _)| id == h.identity())
                {
                    if saved as usize == *total {
                        "In library".into()
                    } else if (saved as usize) < *total {
                        format!("{saved} of {total} Tracks in library")
                    } else {
                        format!("{saved} Tracks in library")
                    }
                } else {
                    format!("{saved} Tracks in library")
                };
                map([
                    ("title", string(h.title())),
                    ("context", string(context)),
                    ("section", string(section)),
                    ("membership", string(membership)),
                ])
                .into()
            })
            .collect();
        let mut tracks = QVariantList::default();
        let mut title = String::new();
        let mut artist = String::new();
        let mut date = String::new();
        let mut total = 0;
        if let Some(r) = &state.page.detail {
            title = r.album.title.clone();
            artist = credit_display(&r.album.credits);
            date = r.album.date.clone();
            for medium in &r.media {
                for track in &medium.tracks {
                    total += 1;
                    if state
                        .page
                        .song
                        .as_ref()
                        .is_some_and(|s| !track.identities.contains(&s.identity))
                    {
                        continue;
                    }
                    let saved = state.saved.contains(&(medium.position, track.position));
                    tracks.push(
                        map([
                            ("title", string(&track.title)),
                            ("artist", string(credit_display(&track.credits))),
                            (
                                "position",
                                string(format!("{}.{}", medium.position, track.position)),
                            ),
                            (
                                "key",
                                string(format!("{}:{}", medium.position, track.position)),
                            ),
                            ("saved", saved.into()),
                        ])
                        .into(),
                    );
                }
            }
        }
        let complete = total > 0 && state.saved.len() == total;
        let heading = state
            .page
            .artist
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or(title);
        map([
            ("busy", (state.pending > 0).into()),
            ("status", string(&state.status)),
            ("hits", hits.into()),
            ("tracks", tracks.into()),
            ("detail", state.page.detail.is_some().into()),
            ("song", state.page.song.is_some().into()),
            ("heading", string(heading)),
            ("artist", string(artist)),
            ("date", string(date)),
            ("back", (!state.history.is_empty()).into()),
            ("more", state.page.next.is_some().into()),
            ("complete", complete.into()),
            (
                "membership",
                string(if total > 0 {
                    format!("{} of {total} Tracks in library", state.saved.len())
                } else {
                    String::new()
                }),
            ),
        ])
    }
}
fn map<const N: usize>(pairs: [(&str, QVariant); N]) -> QVariantMap {
    pairs.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use music_library::domain::ExternalIdentity;
    use std::sync::mpsc;
    fn id(kind: &str, value: &str) -> ExternalIdentity {
        ExternalIdentity {
            provider: "fixture".into(),
            kind: kind.into(),
            external_id: value.into(),
        }
    }
    fn album() -> AlbumCandidate {
        AlbumCandidate {
            identity: id("album", "album"),
            title: "T H E".into(),
            artist: "tricot".into(),
            credits: vec![Credit {
                identity: Some(id("artist", "artist")),
                name: "tricot".into(),
                join_phrase: String::new(),
            }],
            date: "2013".into(),
            primary_type: "Album".into(),
            secondary_types: vec![],
            comment: String::new(),
            score: None,
        }
    }
    fn artist(name: &str) -> ArtistCandidate {
        ArtistCandidate {
            identity: id("artist", "artist"),
            name: name.into(),
            aliases: vec![],
            comment: String::new(),
            country: String::new(),
            artist_type: String::new(),
            score: None,
        }
    }
    struct Fake {
        calls: mpsc::Sender<String>,
        fail: bool,
    }
    impl Fake {
        fn call(&self, kind: &str) -> Result<(), CatalogError> {
            self.calls.send(kind.into()).unwrap();
            if self.fail {
                Err(CatalogError::ServiceUnavailable {
                    message: "unavailable".into(),
                    retry_after: None,
                })
            } else {
                Ok(())
            }
        }
    }
    impl CatalogProvider for Fake {
        fn search_artists(&mut self, q: &str) -> Result<Page<ArtistCandidate>, CatalogError> {
            self.call("artist")?;
            if q == "slow" {
                std::thread::sleep(std::time::Duration::from_millis(220));
            }
            Ok(Page {
                items: vec![artist(if q == "slow" { "slow" } else { "tricot" })],
                next_offset: None,
            })
        }
        fn search_albums(&mut self, _: &str, _: u32) -> Result<Page<AlbumCandidate>, CatalogError> {
            self.call("album")?;
            Ok(Page {
                items: vec![album()],
                next_offset: None,
            })
        }
        fn catalog_songs(&mut self, _: &str) -> Result<Page<SongCandidate>, CatalogError> {
            self.call("song")?;
            Ok(Page {
                items: vec![SongCandidate {
                    identity: id("track", "one"),
                    title: "Hatsumimi".into(),
                    artist: "tricot".into(),
                    album: album(),
                    release: None,
                    disc: Some(1),
                    position: Some(1),
                }],
                next_offset: None,
            })
        }
        fn browse_artist(
            &mut self,
            _: &ExternalIdentity,
            _: u32,
        ) -> Result<Page<AlbumCandidate>, CatalogError> {
            self.call("browse")?;
            Ok(Page {
                items: vec![album()],
                next_offset: None,
            })
        }
        fn catalog_album(&mut self, _: &AlbumCandidate) -> Result<Release, CatalogError> {
            self.call("detail")?;
            let a = album();
            Ok(Release {
                album: a.album(),
                identity: id("release", "release"),
                identities: vec![],
                title: a.title,
                date: a.date,
                credits: a.credits.clone(),
                media: vec![
                    Medium {
                        position: 1,
                        tracks: vec![Track {
                            position: 1,
                            title: "Hatsumimi".into(),
                            credits: a.credits.clone(),
                            identities: vec![id("track", "one")],
                        }],
                    },
                    Medium {
                        position: 2,
                        tracks: vec![Track {
                            position: 1,
                            title: "Second Song".into(),
                            credits: a.credits,
                            identities: vec![id("track", "two")],
                        }],
                    },
                ],
            })
        }
        fn releases(
            &mut self,
            _: &ExternalIdentity,
            _: u32,
        ) -> Result<Page<ReleaseCandidate>, CatalogError> {
            unreachable!()
        }
        fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
            self.catalog_album(&album())
        }
    }
    #[test]
    fn catalog_rows_consolidate_only_established_application_identities() {
        let temp = tempfile::tempdir().unwrap();
        let mut library = music_library::Library::open(temp.path().join("catalog.sqlite")).unwrap();
        let (calls, _recv) = mpsc::channel();
        let release = Fake { calls, fail: false }.catalog_album(&album()).unwrap();
        let imported = library.add_catalog_release(&release).unwrap();
        let local = library.album_for_release(&imported.release_id).unwrap();
        let mut alias = album();
        alias.identity.provider = "another".into();
        library
            .attach_album_external_identity(&local.album_id, &alias.identity)
            .unwrap();
        let mut unrelated = album();
        unrelated.identity.external_id = "unrelated".into();
        let mut b = Bridge::new(crate::session::Session::new(library));
        b.add_music.page.hits = vec![
            Hit::Album(album()),
            Hit::Album(alias),
            Hit::Album(unrelated),
        ];
        b.music_context().unwrap();
        assert_eq!(b.add_music.page.hits.len(), 2);
        assert_eq!(b.add_music.contexts[0].saved, 2);
        assert_eq!(b.add_music.contexts[1].saved, 0);
    }
    #[test]
    fn worker_coalesces_queries_and_stops_obsolete_category_requests() {
        let (calls, recv) = mpsc::channel();
        let (send, replies) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(1));
        let worker = Worker::new(
            "fixture".into(),
            Box::new(Fake { calls, fail: false }),
            generation.clone(),
            move |g, r| {
                send.send((g, r)).unwrap();
            },
        );
        worker.send(1, Request::Search("slow".into(), Kind::All));
        assert_eq!(
            recv.recv_timeout(std::time::Duration::from_secs(1))
                .unwrap(),
            "artist"
        );
        generation.store(2, Ordering::Relaxed);
        worker.send(2, Request::Search("obsolete".into(), Kind::All));
        generation.store(3, Ordering::Relaxed);
        worker.send(3, Request::Search("tricot".into(), Kind::All));
        loop {
            let (g, reply) = replies
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            assert_eq!(g, 3);
            if matches!(reply, Reply::Done) {
                break;
            }
        }
        assert_eq!(
            recv.try_iter().collect::<Vec<_>>(),
            vec!["artist", "album", "song"]
        );
    }

    #[test]
    fn interactive_add_music_qml() {
        let temp = tempfile::tempdir().unwrap();
        let library = music_library::Library::open(temp.path().join("catalog.sqlite")).unwrap();
        let bridge =
            qmetaobject::QObjectBox::new(Bridge::new(crate::session::Session::new(library)));
        let mut engine = qmetaobject::QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let (calls, recv) = mpsc::channel();
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.auto_match = false;
            for (name, fail) in [("fixture", false), ("unavailable", true)] {
                let pointer = QPointer::from(&**b);
                let callback = qmetaobject::queued_callback(move |(g, r)| {
                    if let Some(b) = pointer.as_pinned() {
                        b.borrow_mut().music_reply(g, r);
                    }
                });
                let generation = b.add_music.generation.clone();
                b.add_music.workers.push(Worker::new(
                    name.into(),
                    Box::new(Fake {
                        calls: calls.clone(),
                        fail,
                    }),
                    generation,
                    move |g, r| callback((g, r)),
                ));
            }
        }
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../AddMusicTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method("exerciseAddMusic".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        // Exercise the Album action with genuinely missing membership as well
        // as the complete-Album no-op exercised through QML above.
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            let release = b.add_music.page.detail.clone().unwrap();
            let track = b
                .session
                .library
                .resolve_tracks_external_identity(&release.media[0].tracks[0].identities[0])
                .unwrap()
                .remove(0);
            b.session.library.remove_from_library(&track).unwrap();
            b.music_context().unwrap();
            assert_eq!(b.add_music.saved.len(), 1);
            b.music_action_inner("album", "").unwrap();
            assert_eq!(b.add_music.saved.len(), 2);
        }
        assert!(recv.try_iter().any(|call| call == "browse"));
        assert!(bridge.pinned().borrow().catalog.worker.is_none());
        assert!(bridge.pinned().borrow().spotify_album_matcher.is_none());
    }
}
