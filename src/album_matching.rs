//! Best-effort post-import enrichment. The worker never owns an import transaction.
use crate::storage::Result;
use crate::{
    Library,
    catalog::{ArtistAlbumCandidate, ArtistCandidate, CatalogProvider, Page},
    domain::{AlbumId, ArtistId, ExternalIdentity, ImportedRelease},
    matching::{normalize, usable},
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatchOutcome {
    Disabled,
    Pending,
    Matched(ExternalIdentity),
    MatchedClose(ExternalIdentity),
    ArtistAmbiguous(Vec<ArtistCandidate>),
    AlbumAmbiguous(Vec<ArtistAlbumCandidate>),
    AlreadyMatched,
    Skipped,
    NoConfidentMatch,
    Error(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchInput {
    pub album_id: AlbumId,
    pub title: String,
    pub artist: String,
    pub artist_id: ArtistId,
    pub known_artist: Option<ExternalIdentity>,
}
#[derive(Clone, Debug)]
pub enum Preparation {
    Ready(MatchInput),
    Done(MatchOutcome),
}
#[derive(Debug)]
pub struct MatchReply {
    pub input: MatchInput,
    /// Independently established before Album comparison; retained even on Album errors.
    pub artist: Option<ExternalIdentity>,
    pub outcome: MatchOutcome,
}

pub fn resolve_artist(
    name: &str,
    page: &Page<ArtistCandidate>,
) -> std::result::Result<ExternalIdentity, MatchOutcome> {
    let mut candidates = Vec::new();
    for candidate in &page.items {
        if candidate.identity.provider == "musicbrainz"
            && candidate.identity.kind == "artist"
            && !candidate.identity.external_id.is_empty()
            && normalize(&candidate.name) == normalize(name)
            && !candidates
                .iter()
                .any(|c: &ArtistCandidate| c.identity == candidate.identity)
        {
            candidates.push(candidate.clone());
        }
    }
    if page.next_offset.is_some() || candidates.len() > 1 {
        return Err(MatchOutcome::ArtistAmbiguous(candidates));
    }
    candidates
        .pop()
        .map(|c| c.identity)
        .ok_or(MatchOutcome::NoConfidentMatch)
}

/// Unicode scalar edit distance <= 1, without deleting punctuation or semantic words.
pub fn close_album_title(left: &str, right: &str) -> bool {
    let a = normalize(left).chars().collect::<Vec<_>>();
    let b = normalize(right).chars().collect::<Vec<_>>();
    if a.len() < 5 || b.len() < 5 || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (mut i, mut j, mut edits) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
            continue;
        }
        edits += 1;
        if edits > 1 {
            return false;
        }
        if a.len() >= b.len() {
            i += 1;
        }
        if b.len() >= a.len() {
            j += 1;
        }
    }
    edits + usize::from(i < a.len() || j < b.len()) <= 1
}

pub fn accepted_album(
    title: &str,
    artist: &ExternalIdentity,
    page: &Page<ArtistAlbumCandidate>,
) -> MatchOutcome {
    if artist.provider != "musicbrainz" || artist.kind != "artist" || artist.external_id.is_empty()
    {
        return MatchOutcome::NoConfidentMatch;
    }
    let mut candidates = Vec::new();
    for candidate in &page.items {
        // Initial automatic path supports one Artist at both ends, not collaborations.
        if candidate.artist_ids.as_slice() == std::slice::from_ref(artist)
            && candidate.identity.provider == "musicbrainz"
            && candidate.identity.kind == "release_group"
            && !candidate.identity.external_id.is_empty()
            && !candidates
                .iter()
                .any(|c: &ArtistAlbumCandidate| c.identity == candidate.identity)
        {
            candidates.push(candidate.clone());
        }
    }
    let exact = candidates
        .iter()
        .filter(|c| normalize(&c.title) == normalize(title))
        .cloned()
        .collect::<Vec<_>>();
    let (plausible, close) = if exact.is_empty() {
        (
            candidates
                .into_iter()
                .filter(|c| close_album_title(title, &c.title))
                .collect::<Vec<_>>(),
            true,
        )
    } else {
        (exact, false)
    };
    if page.next_offset.is_some() || plausible.len() > 1 {
        return MatchOutcome::AlbumAmbiguous(plausible);
    }
    match plausible.into_iter().next() {
        Some(c) if close => MatchOutcome::MatchedClose(c.identity),
        Some(c) => MatchOutcome::Matched(c.identity),
        None => MatchOutcome::NoConfidentMatch,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AutoMatchPolicy {
    pub enabled: bool,
}
impl Default for AutoMatchPolicy {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// One owned serial worker, session-only deduplication and explicit completion on
/// the application owner thread. Drop cancels queued work and joins in-flight HTTP.
pub struct AlbumMatcher {
    sender: Option<mpsc::Sender<MatchInput>>,
    thread: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    outcomes: HashMap<AlbumId, MatchOutcome>,
}
impl AlbumMatcher {
    pub fn new(
        mut provider: impl CatalogProvider + 'static,
        emit: impl Fn(MatchReply) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::channel::<MatchInput>();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = thread::Builder::new()
            .name("album-matching".into())
            .spawn(move || {
                // Cache only independently resolved, complete, exact-name Artist searches.
                // Bounded session cache; distinct local Artist rows are not merged.
                let mut artists = HashMap::<String, ExternalIdentity>::new();
                while let Ok(input) = receiver.recv() {
                    if stopped.load(Ordering::Acquire) {
                        break;
                    }
                    let resolved = if let Some(id) = input
                        .known_artist
                        .clone()
                        .or_else(|| artists.get(&input.artist).cloned())
                    {
                        Ok(id)
                    } else {
                        provider
                            .search_artists(&input.artist)
                            .map_err(|e| MatchOutcome::Error(e.to_string()))
                            .and_then(|page| resolve_artist(&input.artist, &page))
                            .inspect(|id| {
                                if artists.len() == 64 {
                                    artists.clear();
                                }
                                artists.insert(input.artist.clone(), id.clone());
                            })
                    };
                    let (artist, outcome) = match resolved {
                        Err(outcome) => (None, outcome),
                        Ok(id) => {
                            let outcome = provider
                                .artist_albums(&id, &input.title)
                                .map(|page| accepted_album(&input.title, &id, &page))
                                .unwrap_or_else(|e| MatchOutcome::Error(e.to_string()));
                            (Some(id), outcome)
                        }
                    };
                    if !stopped.load(Ordering::Acquire) {
                        emit(MatchReply {
                            input,
                            artist,
                            outcome,
                        });
                    }
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            thread: Some(thread),
            stop,
            outcomes: HashMap::new(),
        })
    }
    /// Call only with completed import results. Disabled policy does not even inspect Albums.
    pub fn after_import(
        &mut self,
        library: &Library,
        imports: &[ImportedRelease],
        policy: AutoMatchPolicy,
    ) -> Result<Vec<(AlbumId, MatchOutcome)>> {
        if !policy.enabled {
            return Ok(vec![]);
        }
        let mut seen = HashSet::new();
        let mut updates = Vec::new();
        for imported in imports {
            let id = library.album_for_release(&imported.release_id)?.album_id;
            if seen.insert(id.clone()) {
                let outcome = if let Some(outcome) = self.outcomes.get(&id) {
                    outcome.clone()
                } else {
                    self.match_album(library, &id)?
                };
                updates.push((id, outcome));
            }
        }
        Ok(updates)
    }
    /// Manual retry is independent of automatic policy. Pending requests coalesce.
    pub fn match_album(&mut self, library: &Library, id: &AlbumId) -> Result<MatchOutcome> {
        if self.outcomes.get(id) == Some(&MatchOutcome::Pending) {
            return Ok(MatchOutcome::Pending);
        }
        let outcome = match library.prepare_album_match(id)? {
            Preparation::Done(outcome) => outcome,
            Preparation::Ready(input) => match self.sender.as_ref().unwrap().send(input) {
                Ok(()) => MatchOutcome::Pending,
                Err(error) => MatchOutcome::Error(error.to_string()),
            },
        };
        self.outcomes.insert(id.clone(), outcome.clone());
        Ok(outcome)
    }
    /// Explicit diagnostic/manual choice from the retained Artist candidates.
    /// No entity merging or local-name rewrite; the following Album request stays scoped.
    pub fn select_artist(
        &mut self,
        library: &mut Library,
        album: &AlbumId,
        index: usize,
    ) -> Result<MatchOutcome> {
        let Some(MatchOutcome::ArtistAmbiguous(candidates)) = self.outcomes.get(album) else {
            return Err(crate::storage::Error::Invalid(
                "No Artist choices for this Album".into(),
            ));
        };
        let candidate = candidates
            .get(index)
            .ok_or_else(|| crate::storage::Error::Invalid("Stale Artist selection".into()))?
            .clone();
        let input = match library.prepare_album_match(album)? {
            Preparation::Ready(input) => input,
            Preparation::Done(outcome) => return Ok(outcome),
        };
        if normalize(&candidate.name) != input.artist {
            return Ok(MatchOutcome::Skipped);
        }
        let outcome = library.complete_album_match(MatchReply {
            input,
            artist: Some(candidate.identity),
            outcome: MatchOutcome::NoConfidentMatch,
        })?;
        self.outcomes.insert(album.clone(), outcome.clone());
        if outcome != MatchOutcome::NoConfidentMatch {
            return Ok(outcome);
        }
        self.match_album(library, album)
    }
    /// Revalidate eligibility/metadata and existing identity before the short attachment transaction.
    pub fn complete(&mut self, library: &mut Library, reply: MatchReply) -> MatchOutcome {
        let id = reply.input.album_id.clone();
        let outcome = library
            .complete_album_match(reply)
            .unwrap_or_else(|e| MatchOutcome::Error(e.to_string()));
        self.outcomes.insert(id, outcome.clone());
        outcome
    }
}
impl Drop for AlbumMatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(crate) fn eligible_text(title: &str, artist: &str) -> bool {
    usable(title) && usable(artist)
}
