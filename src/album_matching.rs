//! Best-effort post-import enrichment. The worker never owns an import transaction.
use crate::storage::Result;
use crate::{
    Library,
    catalog::{ArtistAlbumCandidate, ArtistCandidate, CatalogProvider, Page},
    domain::{AlbumId, ArtistId, ExternalIdentity, ImportedRelease},
    matching::{normalize, usable},
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
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
    Deferred(crate::catalog::CatalogError),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchInput {
    pub album_id: AlbumId,
    pub title: String,
    pub artist: String,
    pub artist_id: ArtistId,
    pub known_artist: Option<ExternalIdentity>,
    pub manual_artist: bool,
}
#[derive(Clone, Debug)]
pub enum Preparation {
    Ready(MatchInput),
    Done(MatchOutcome),
}
#[derive(Debug)]
pub struct MatchReply {
    pub input: MatchInput,
    /// Established independently (retained on Album errors), or by a unique pair.
    pub artist: Option<ExternalIdentity>,
    pub outcome: MatchOutcome,
    /// Selected provider presentation; never written to library metadata.
    pub matched_album: Option<ArtistAlbumCandidate>,
}

fn matched_album(
    outcome: &MatchOutcome,
    page: &Page<ArtistAlbumCandidate>,
) -> Option<ArtistAlbumCandidate> {
    let (MatchOutcome::Matched(identity) | MatchOutcome::MatchedClose(identity)) = outcome else {
        return None;
    };
    page.items.iter().find(|c| &c.identity == identity).cloned()
}

pub fn resolve_artist(
    name: &str,
    page: &Page<ArtistCandidate>,
) -> std::result::Result<ExternalIdentity, MatchOutcome> {
    let key = normalize(name);
    let mut candidates = Vec::<ArtistCandidate>::new();
    for candidate in &page.items {
        if candidate.identity.provider == "musicbrainz"
            && candidate.identity.kind == "artist"
            && !candidate.identity.external_id.is_empty()
            && !candidates.iter().any(|c| c.identity == candidate.identity)
        {
            candidates.push(candidate.clone());
        }
    }
    let names = |c: &ArtistCandidate| {
        std::iter::once(c.name.as_str())
            .chain(c.aliases.iter().map(String::as_str))
            .map(normalize)
            .collect::<Vec<_>>()
    };
    let exact = candidates
        .iter()
        .filter(|c| names(c).contains(&key))
        .cloned()
        .collect::<Vec<_>>();
    if exact.len() > 1 {
        return Err(MatchOutcome::ArtistAmbiguous(exact));
    }
    let Some(winner) = exact.first() else {
        return Err(MatchOutcome::NoConfidentMatch);
    };
    if normalize(&winner.name) != key {
        let competitors = candidates
            .iter()
            .filter(|c| {
                c.identity != winner.identity && names(c).iter().any(|n| edit_close(&key, n, 1, 1))
            })
            .cloned()
            .collect::<Vec<_>>();
        if !competitors.is_empty() {
            return Err(MatchOutcome::ArtistAmbiguous(
                exact.into_iter().chain(competitors).collect(),
            ));
        }
    }
    // A bounded Artist page alone cannot establish uniqueness, but its plausible
    // candidates may still be jointly resolved by an Album-scoped search.
    if page.next_offset.is_some() {
        return Err(MatchOutcome::ArtistAmbiguous(exact));
    }
    Ok(winner.identity.clone())
}

/// Album evidence can disambiguate the supplied plausible candidates, never add
/// an Artist. Close-name veto candidates can block acceptance but cannot win it.
pub fn corroborate_artist_album(
    local_artist: &str,
    title: &str,
    candidates: &[ArtistCandidate],
    page: &Page<ArtistAlbumCandidate>,
) -> (Option<ExternalIdentity>, MatchOutcome) {
    if page.next_offset.is_some() {
        return (None, MatchOutcome::ArtistAmbiguous(candidates.to_vec()));
    }
    let mut evidence = candidates
        .iter()
        .map(|candidate| {
            let outcome = accepted_album(title, &candidate.identity, page);
            let supported = matches!(
                outcome,
                MatchOutcome::Matched(_)
                    | MatchOutcome::MatchedClose(_)
                    | MatchOutcome::AlbumAmbiguous(_)
            );
            (candidate.clone(), outcome, supported)
        })
        .collect::<Vec<_>>();
    let supported = evidence
        .iter()
        .filter(|(_, _, supported)| *supported)
        .collect::<Vec<_>>();
    if let [(artist, outcome, _)] = supported.as_slice() {
        let exact_name = normalize(&artist.name) == normalize(local_artist)
            || artist
                .aliases
                .iter()
                .any(|name| normalize(name) == normalize(local_artist));
        if exact_name
            && matches!(
                outcome,
                MatchOutcome::Matched(_) | MatchOutcome::MatchedClose(_)
            )
        {
            return (Some(artist.identity.clone()), outcome.clone());
        }
    }
    // Helpful presentation only: supported candidates first, then stable MBID order.
    evidence.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| a.0.identity.external_id.cmp(&b.0.identity.external_id))
    });
    (
        None,
        MatchOutcome::ArtistAmbiguous(evidence.into_iter().map(|v| v.0).collect()),
    )
}

/// Bounded Unicode Levenshtein comparison. No transposition or semantic normalization.
fn edit_close(left: &str, right: &str, limit: usize, minimum: usize) -> bool {
    let a: Vec<_> = normalize(left).chars().collect();
    let b: Vec<_> = normalize(right).chars().collect();
    if a.len() < minimum || b.len() < minimum || a.len().abs_diff(b.len()) > limit {
        return false;
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ac) in a.iter().enumerate() {
        let mut current = vec![limit + 1; b.len() + 1];
        current[0] = i + 1;
        for j in i.saturating_sub(limit)..b.len().min(i + limit + 1) {
            current[j + 1] = (previous[j] + usize::from(*ac != b[j]))
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        if *current.iter().min().unwrap() > limit {
            return false;
        }
        previous = current;
    }
    previous[b.len()] <= limit
}
fn qualifiers(title: &str) -> Vec<String> {
    let normalized = normalize(title);
    let words = normalized
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    // A standalone title such as Acoustic -> Acoustics is not a version suffix.
    if words.len() < 2 {
        return vec![];
    }
    words
        .into_iter()
        .filter(|w| {
            matches!(
                *w,
                "live"
                    | "remix"
                    | "remixed"
                    | "acoustic"
                    | "deluxe"
                    | "remaster"
                    | "remastered"
                    | "demo"
                    | "edit"
            )
        })
        .map(str::to_owned)
        .collect()
}
pub fn close_album_title(left: &str, right: &str) -> bool {
    qualifiers(left) == qualifiers(right) && edit_close(left, right, 1, 5)
}
/// Comparison-only trailing tag decorations; EP/LP require candidate type evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlbumTitleVariant {
    pub title: String,
    pub requires_ep: bool,
    pub requires_album: bool,
}
pub fn album_title_variants(title: &str) -> Vec<AlbumTitleVariant> {
    let key = normalize(title);
    let mut result = vec![];
    for suffix in [
        "ep", "lp", "cd", "cd1", "cd 1", "cd2", "cd 2", "disc 1", "disc 2", "2cd", "2xcd",
    ] {
        for ending in [
            format!(" - {suffix}"),
            format!(" ({suffix})"),
            format!(" [{suffix}]"),
            format!(" {suffix}"),
        ] {
            if let Some(base) = key
                .strip_suffix(&ending)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                // Do not also interpret a dash-delimited suffix as a plain-space suffix.
                if base.ends_with(" -") {
                    continue;
                }
                let variant = AlbumTitleVariant {
                    title: base.into(),
                    requires_ep: suffix == "ep",
                    requires_album: suffix == "lp",
                };
                if !result.contains(&variant) {
                    result.push(variant);
                }
            }
        }
    }
    result
}
pub fn accepted_album(
    title: &str,
    artist: &ExternalIdentity,
    page: &Page<ArtistAlbumCandidate>,
) -> MatchOutcome {
    accepted_album_confirmed(title, artist, page, false)
}
pub fn accepted_album_confirmed(
    title: &str,
    artist: &ExternalIdentity,
    page: &Page<ArtistAlbumCandidate>,
    manual: bool,
) -> MatchOutcome {
    if artist.provider != "musicbrainz" || artist.kind != "artist" || artist.external_id.is_empty()
    {
        return MatchOutcome::NoConfidentMatch;
    }
    let mut candidates = Vec::<ArtistAlbumCandidate>::new();
    for c in &page.items {
        if c.artist_ids.as_slice() == std::slice::from_ref(artist)
            && c.identity.provider == "musicbrainz"
            && c.identity.kind == "release_group"
            && !c.identity.external_id.is_empty()
            && !candidates.iter().any(|v| v.identity == c.identity)
        {
            candidates.push(c.clone());
        }
    }
    let exact = candidates
        .iter()
        .filter(|c| normalize(&c.title) == normalize(title))
        .cloned()
        .collect::<Vec<_>>();
    // Literal format/type words may be part of the real title. Only derive
    // fallback variants when the complete title has no exact candidate.
    let variants = if exact.is_empty() {
        album_title_variants(title)
    } else {
        vec![]
    };
    let structural = candidates
        .iter()
        .filter(|c| {
            variants.iter().any(|v| {
                v.title == normalize(&c.title)
                    && (!v.requires_ep || normalize(&c.primary_type) == "ep")
                    && (!v.requires_album || normalize(&c.primary_type) == "album")
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    let (plausible, close) = if !exact.is_empty() {
        (exact, false)
    } else if !structural.is_empty() {
        (structural, true)
    } else {
        (
            candidates
                .into_iter()
                .filter(|c| {
                    close_album_title(title, &c.title)
                        || (manual
                            && qualifiers(title) == qualifiers(&c.title)
                            && edit_close(title, &c.title, 2, 8))
                })
                .collect(),
            true,
        )
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CircuitState {
    Available,
    Unavailable(crate::catalog::CatalogError),
}
fn provider_outcome(error: crate::catalog::CatalogError) -> MatchOutcome {
    if error.is_provider_unavailable() {
        MatchOutcome::Deferred(error)
    } else {
        MatchOutcome::Error(error.to_string())
    }
}

/// Owner-thread callback token and diagnostic delay; expiry comes from the worker.
#[derive(Clone, Copy, Debug)]
pub struct RetrySchedule {
    pub token: u64,
    pub delay: Duration,
}
#[derive(Default)]
struct Cooldown {
    failures: u32,
    generation: u64,
    scheduled: Option<RetrySchedule>,
}
impl Cooldown {
    fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.scheduled = None;
    }
    fn reset(&mut self) {
        self.cancel();
        self.failures = 0;
    }
    fn arm(&mut self, error: &crate::catalog::CatalogError, now: SystemTime) -> RetrySchedule {
        self.cancel();
        let base = Duration::from_secs(15 << self.failures.min(3));
        self.failures = self.failures.saturating_add(1);
        let header = match error {
            crate::catalog::CatalogError::ServiceUnavailable { retry_after, .. } => {
                retry_after.as_deref()
            }
            _ => None,
        };
        let requested = header.and_then(|s| {
            s.trim()
                .parse::<u64>()
                .ok()
                .map(Duration::from_secs)
                .or_else(|| {
                    httpdate::parse_http_date(s.trim())
                        .ok()?
                        .duration_since(now)
                        .ok()
                })
        });
        let delay = base.max(requested.unwrap_or_default().min(Duration::from_secs(120)));
        let schedule = RetrySchedule {
            token: self.generation,
            delay,
        };
        self.scheduled = Some(schedule);
        schedule
    }
}
enum WorkerCommand {
    Match(MatchInput),
    Recordings(crate::recording::Input),
    Arm { token: u64, deadline: Instant },
    Cancel,
}
#[derive(Clone)]
enum Work {
    Album(AlbumId),
    Recordings(AlbumId),
}
impl Work {
    fn album_id(&self) -> &AlbumId {
        match self {
            Self::Album(id) | Self::Recordings(id) => id,
        }
    }
}
#[derive(Default)]
struct WorkerTimer(Option<(u64, Instant)>);
impl WorkerTimer {
    fn due(&mut self, now: Instant) -> Option<u64> {
        if self.0.is_some_and(|(_, deadline)| now >= deadline) {
            self.0.take().map(|v| v.0)
        } else {
            None
        }
    }
    fn remaining(&self, now: Instant) -> Option<Duration> {
        self.0
            .map(|(_, deadline)| deadline.saturating_duration_since(now))
    }
}

/// One owned serial worker, session-only deduplication and explicit completion on
/// the application owner thread. Drop cancels queued work and joins in-flight HTTP.
pub struct AlbumMatcher {
    sender: Option<mpsc::Sender<WorkerCommand>>,
    thread: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    outcomes: HashMap<AlbumId, MatchOutcome>,
    matched_albums: HashMap<AlbumId, ArtistAlbumCandidate>,
    queue: VecDeque<Work>,
    recording_enabled: bool,
    recording_outcomes: HashMap<AlbumId, crate::recording::Outcome>,
    active: bool,
    circuit: CircuitState,
    cooldown: Cooldown,
    manual_artists: HashMap<AlbumId, ExternalIdentity>,
}
impl AlbumMatcher {
    /// Both callbacks must enqueue onto the application owner thread. Deliver
    /// replies to `complete` and timer tokens to `cooldown_elapsed`; neither callback
    /// may block the worker waiting for the owner.
    pub fn new(
        provider: impl CatalogProvider + 'static,
        emit: impl Fn(MatchReply) + Send + 'static,
        retry_due: impl Fn(u64) + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::with_callbacks(provider, emit, None, retry_due)
    }
    /// Same owned queue/worker and circuit; Recording completion stays on the owner thread.
    pub fn new_with_recordings(
        provider: impl CatalogProvider + 'static,
        emit: impl Fn(MatchReply) + Send + 'static,
        recordings: impl Fn(crate::recording::Reply) + Send + 'static,
        retry_due: impl Fn(u64) + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::with_callbacks(provider, emit, Some(Box::new(recordings)), retry_due)
    }
    fn with_callbacks(
        mut provider: impl CatalogProvider + 'static,
        emit: impl Fn(MatchReply) + Send + 'static,
        recordings: Option<Box<dyn Fn(crate::recording::Reply) + Send>>,
        retry_due: impl Fn(u64) + Send + 'static,
    ) -> std::io::Result<Self> {
        let recording_enabled = recordings.is_some();
        let (sender, receiver) = mpsc::channel::<WorkerCommand>();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = thread::Builder::new()
            .name("album-matching".into())
            .spawn(move || {
                // Cache only independently resolved, complete primary/alias Artist searches.
                // Bounded session cache; canonical identity reassignment happens on completion.
                let mut artists = HashMap::<String, ExternalIdentity>::new();
                let mut timer = WorkerTimer::default();
                loop {
                    if stopped.load(Ordering::Acquire) {
                        break;
                    }
                    if let Some(token) = timer.due(Instant::now()) {
                        retry_due(token);
                    }
                    // Interruptible wait: commands and shutdown wake it immediately.
                    let command = match timer.remaining(Instant::now()) {
                        Some(delay) => match receiver.recv_timeout(delay) {
                            Ok(command) => command,
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        },
                        None => match receiver.recv() {
                            Ok(command) => command,
                            Err(_) => break,
                        },
                    };
                    if stopped.load(Ordering::Acquire) {
                        break;
                    }
                    let input = match command {
                        WorkerCommand::Arm { token, deadline } => {
                            timer.0 = Some((token, deadline));
                            continue;
                        }
                        WorkerCommand::Cancel => {
                            timer.0 = None;
                            continue;
                        }
                        WorkerCommand::Match(input) => input,
                        WorkerCommand::Recordings(input) => {
                            let result = provider.recordings(&input.group);
                            if !stopped.load(Ordering::Acquire)
                                && let Some(emit) = &recordings
                            {
                                emit(crate::recording::Reply { input, result });
                            }
                            continue;
                        }
                    };
                    let resolved = if let Some(id) = input
                        .known_artist
                        .clone()
                        .or_else(|| artists.get(&input.artist).cloned())
                    {
                        Ok(id)
                    } else {
                        provider
                            .search_artists(&input.artist)
                            .map_err(provider_outcome)
                            .and_then(|page| resolve_artist(&input.artist, &page))
                            .inspect(|id| {
                                if artists.len() == 64 {
                                    artists.clear();
                                }
                                artists.insert(input.artist.clone(), id.clone());
                            })
                    };
                    let mut selected_album = None;
                    let (artist, outcome) = match resolved {
                        Err(MatchOutcome::ArtistAmbiguous(candidates))
                            if !candidates.is_empty() =>
                        {
                            let identities = candidates
                                .iter()
                                .map(|c| c.identity.clone())
                                .collect::<Vec<_>>();
                            match provider.albums_for_artists(&identities, &input.title) {
                                Ok(page) => {
                                    let pair = corroborate_artist_album(
                                        &input.artist,
                                        &input.title,
                                        &candidates,
                                        &page,
                                    );
                                    selected_album = matched_album(&pair.1, &page);
                                    pair
                                }
                                Err(error) => (None, provider_outcome(error)),
                            }
                            // Deliberately not cached by Artist name: this identity depended on this Album.
                        }
                        Err(outcome) => (None, outcome),
                        Ok(id) => {
                            let response = if input.manual_artist {
                                provider.artist_albums_confirmed(&id, &input.title)
                            } else {
                                provider.artist_albums(&id, &input.title)
                            };
                            let outcome = response
                                .map(|page| {
                                    let outcome = accepted_album_confirmed(
                                        &input.title,
                                        &id,
                                        &page,
                                        input.manual_artist,
                                    );
                                    selected_album = matched_album(&outcome, &page);
                                    outcome
                                })
                                .unwrap_or_else(provider_outcome);
                            (Some(id), outcome)
                        }
                    };
                    if !stopped.load(Ordering::Acquire) {
                        emit(MatchReply {
                            input,
                            artist,
                            outcome,
                            matched_album: selected_album,
                        });
                    }
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            thread: Some(thread),
            stop,
            outcomes: HashMap::new(),
            matched_albums: HashMap::new(),
            queue: VecDeque::new(),
            recording_enabled,
            recording_outcomes: HashMap::new(),
            active: false,
            circuit: CircuitState::Available,
            cooldown: Cooldown::default(),
            manual_artists: HashMap::new(),
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
                self.enqueue_recordings(library, &id)?;
                self.dispatch(library, false)?;
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
            Preparation::Ready(input) => {
                if !self.queue.iter().any(|i| i.album_id() == id) {
                    self.queue.push_back(Work::Album(input.album_id));
                }
                MatchOutcome::Pending
            }
        };
        self.outcomes.insert(id.clone(), outcome.clone());
        self.enqueue_recordings(library, id)?;
        self.dispatch(library, false)?;
        Ok(outcome)
    }
    /// Explicit diagnostic/manual choice from the retained Artist candidates.
    /// Strong identity may consolidate Artists; credited names and Album scope stay intact.
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
        let outcome = library.complete_album_match(MatchReply {
            input,
            artist: Some(candidate.identity.clone()),
            outcome: MatchOutcome::NoConfidentMatch,
            matched_album: None,
        })?;
        self.outcomes.insert(album.clone(), outcome.clone());
        if outcome != MatchOutcome::NoConfidentMatch {
            return Ok(outcome);
        }
        self.manual_artists
            .insert(album.clone(), candidate.identity);
        self.match_album(library, album)
    }
    /// Revalidate eligibility/metadata and existing identity before the short attachment transaction.
    pub fn complete(&mut self, library: &mut Library, reply: MatchReply) -> MatchOutcome {
        let id = reply.input.album_id.clone();
        let presentation = reply.matched_album.clone();
        let unavailable = match &reply.outcome {
            MatchOutcome::Deferred(e) => Some(e.clone()),
            _ => None,
        };
        let mut retry = reply.input.clone();
        retry.known_artist = reply.artist.clone().or(retry.known_artist);
        let outcome = library
            .complete_album_match(reply)
            .unwrap_or_else(|e| MatchOutcome::Error(e.to_string()));
        self.active = false;
        self.matched_albums.remove(&id);
        if matches!(
            outcome,
            MatchOutcome::Matched(_) | MatchOutcome::MatchedClose(_)
        ) && let Some(presentation) = presentation
        {
            self.matched_albums.insert(id.clone(), presentation);
        }
        if let Some(error) = unavailable {
            self.circuit = CircuitState::Unavailable(error.clone());
            self.queue.push_front(Work::Album(retry.album_id));
            self.outcomes
                .insert(id, MatchOutcome::Deferred(error.clone()));
            let schedule = self.cooldown.arm(&error, SystemTime::now());
            let _ = self.sender.as_ref().unwrap().send(WorkerCommand::Arm {
                token: schedule.token,
                deadline: Instant::now() + schedule.delay,
            });
            return MatchOutcome::Deferred(error);
        }
        self.circuit = CircuitState::Available;
        self.cooldown.reset();
        let _ = self.sender.as_ref().unwrap().send(WorkerCommand::Cancel);
        self.outcomes.insert(id.clone(), outcome.clone());
        if let Err(error) = self.enqueue_recordings(library, &id) {
            self.recording_outcomes
                .insert(id, crate::recording::Outcome::Error(error.to_string()));
        }
        if let Err(error) = self.dispatch(library, false) {
            return MatchOutcome::Error(error.to_string());
        }
        outcome
    }
    pub fn circuit_state(&self) -> &CircuitState {
        &self.circuit
    }
    pub fn recording_outcome(&self, id: &AlbumId) -> Option<&crate::recording::Outcome> {
        self.recording_outcomes.get(id)
    }
    fn enqueue_recordings(&mut self, library: &Library, id: &AlbumId) -> Result<()> {
        if !self.recording_enabled
            || self.recording_outcomes.get(id) == Some(&crate::recording::Outcome::Pending)
            || self
                .queue
                .iter()
                .any(|w| matches!(w,Work::Recordings(existing) if existing==id))
        {
            return Ok(());
        }
        if library.prepare_recording_match(id)?.is_some() {
            self.queue.push_back(Work::Recordings(id.clone()));
            self.recording_outcomes
                .insert(id.clone(), crate::recording::Outcome::Pending);
        }
        Ok(())
    }
    pub fn complete_recordings(
        &mut self,
        library: &mut Library,
        reply: crate::recording::Reply,
    ) -> crate::recording::Outcome {
        let id = reply.input.album_id.clone();
        let outcome = library
            .complete_recording_match(reply)
            .unwrap_or_else(|e| crate::recording::Outcome::Error(e.to_string()));
        self.active = false;
        self.recording_outcomes.insert(id.clone(), outcome.clone());
        if let crate::recording::Outcome::Deferred(error) = &outcome {
            self.circuit = CircuitState::Unavailable(error.clone());
            self.queue.push_front(Work::Recordings(id));
            let schedule = self.cooldown.arm(error, SystemTime::now());
            let _ = self.sender.as_ref().unwrap().send(WorkerCommand::Arm {
                token: schedule.token,
                deadline: Instant::now() + schedule.delay,
            });
        } else {
            self.circuit = CircuitState::Available;
            self.cooldown.reset();
            let _ = self.sender.as_ref().unwrap().send(WorkerCommand::Cancel);
            if let Err(e) = self.dispatch(library, false) {
                return crate::recording::Outcome::Error(e.to_string());
            }
        }
        outcome
    }
    pub fn pending_count(&self) -> usize {
        self.queue.len() + usize::from(self.active)
    }
    pub fn probe_pending(&self) -> bool {
        self.active && matches!(self.circuit, CircuitState::Unavailable(_))
    }
    pub fn outcome(&self, id: &AlbumId) -> Option<&MatchOutcome> {
        self.outcomes.get(id)
    }
    /// Session-only diagnostic metadata, exposed only for a current successful match.
    pub fn matched_album(&self, id: &AlbumId) -> Option<&ArtistAlbumCandidate> {
        if matches!(
            self.outcomes.get(id),
            Some(MatchOutcome::Matched(_) | MatchOutcome::MatchedClose(_))
        ) {
            self.matched_albums.get(id)
        } else {
            None
        }
    }

    pub fn retry_schedule(&self) -> Option<RetrySchedule> {
        self.cooldown.scheduled
    }
    /// Deliver the worker's timer callback on the application owner thread.
    /// Stale callbacks (including those queued before manual retry) are harmless.
    pub fn cooldown_elapsed(&mut self, library: &Library, token: u64) -> Result<()> {
        if self.cooldown.scheduled.is_some_and(|s| s.token == token) {
            self.retry_catalog_matching(library)?;
        }
        Ok(())
    }
    /// Immediate single probe; invalidate the scheduled automatic callback first.
    pub fn retry_catalog_matching(&mut self, library: &Library) -> Result<()> {
        if matches!(self.circuit, CircuitState::Unavailable(_)) && !self.active {
            self.cooldown.cancel();
            let _ = self.sender.as_ref().unwrap().send(WorkerCommand::Cancel);
            self.dispatch(library, true)?;
        }
        Ok(())
    }
    fn dispatch(&mut self, library: &Library, probe: bool) -> Result<()> {
        if self.active || (!probe && matches!(self.circuit, CircuitState::Unavailable(_))) {
            return Ok(());
        }
        while let Some(work) = self.queue.front().cloned() {
            if let Work::Recordings(id) = &work {
                if let Some(input) = library.prepare_recording_match(id)? {
                    self.sender
                        .as_ref()
                        .unwrap()
                        .send(WorkerCommand::Recordings(input))
                        .map_err(|e| crate::storage::Error::Invalid(e.to_string()))?;
                    self.queue.pop_front();
                    self.active = true;
                    self.recording_outcomes
                        .insert(id.clone(), crate::recording::Outcome::Pending);
                    break;
                }
                self.queue.pop_front();
                self.recording_outcomes
                    .insert(id.clone(), crate::recording::Outcome::Complete(vec![]));
                continue;
            }
            // Refresh identities after prior completions/consolidation or manual selection.
            let preparation = library.prepare_album_match(work.album_id())?;
            let id = work.album_id().clone();
            match preparation {
                Preparation::Done(outcome) => {
                    self.queue.pop_front();
                    self.outcomes.insert(id.clone(), outcome);
                    self.enqueue_recordings(library, &id)?;
                }
                Preparation::Ready(mut input) => {
                    input.manual_artist = self
                        .manual_artists
                        .get(&id)
                        .is_some_and(|identity| input.known_artist.as_ref() == Some(identity));
                    self.sender
                        .as_ref()
                        .unwrap()
                        .send(WorkerCommand::Match(input))
                        .map_err(|e| crate::storage::Error::Invalid(e.to_string()))?;
                    self.queue.pop_front();
                    self.active = true;
                    self.outcomes.insert(id, MatchOutcome::Pending);
                    break;
                }
            }
        }
        Ok(())
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

#[cfg(test)]
mod cooldown_tests {
    use super::*;
    use crate::catalog::CatalogError;
    fn unavailable(header: Option<&str>) -> CatalogError {
        CatalogError::ServiceUnavailable {
            message: "503".into(),
            retry_after: header.map(str::to_owned),
        }
    }
    struct IdleProvider;
    impl crate::catalog::CatalogProvider for IdleProvider {
        fn search_albums(
            &mut self,
            _: &str,
            _: u32,
        ) -> std::result::Result<crate::catalog::Page<crate::catalog::AlbumCandidate>, CatalogError>
        {
            unreachable!()
        }
        fn releases(
            &mut self,
            _: &ExternalIdentity,
            _: u32,
        ) -> std::result::Result<crate::catalog::Page<crate::catalog::ReleaseCandidate>, CatalogError>
        {
            unreachable!()
        }
        fn release(
            &mut self,
            _: &ExternalIdentity,
        ) -> std::result::Result<crate::catalog::Release, CatalogError> {
            unreachable!()
        }
    }
    #[test]
    fn worker_delivers_expiry_once_and_shutdown_interrupts_long_cooldown() {
        let (send, receive) = mpsc::channel();
        let matcher = AlbumMatcher::new(
            IdleProvider,
            |_| panic!("unexpected request"),
            move |token| {
                send.send(token).unwrap();
            },
        )
        .unwrap();
        matcher
            .sender
            .as_ref()
            .unwrap()
            .send(WorkerCommand::Arm {
                token: 1,
                deadline: Instant::now(),
            })
            .unwrap();
        assert_eq!(receive.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
        assert!(receive.try_recv().is_err());
        matcher
            .sender
            .as_ref()
            .unwrap()
            .send(WorkerCommand::Arm {
                token: 2,
                deadline: Instant::now() + Duration::from_secs(120),
            })
            .unwrap();
        let start = Instant::now();
        drop(matcher);
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(receive.try_recv().is_err());
    }
    #[test]
    fn backoff_caps_resets_and_retry_after_is_bounded() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let mut cooldown = Cooldown::default();
        for expected in [15, 30, 60, 120, 120, 120] {
            assert_eq!(
                cooldown.arm(&unavailable(None), now).delay.as_secs(),
                expected
            );
        }
        cooldown.reset();
        assert_eq!(cooldown.arm(&unavailable(None), now).delay.as_secs(), 15);
        for (header, expected) in [
            ("0", 15),
            ("45", 45),
            ("999999999", 120),
            ("broken", 15),
            ("-1", 15),
        ] {
            let mut cooldown = Cooldown::default();
            assert_eq!(
                cooldown
                    .arm(&unavailable(Some(header)), now)
                    .delay
                    .as_secs(),
                expected
            );
        }
        let date = httpdate::fmt_http_date(now + Duration::from_secs(80));
        assert_eq!(
            Cooldown::default()
                .arm(&unavailable(Some(&date)), now)
                .delay
                .as_secs(),
            80
        );
        let past = httpdate::fmt_http_date(now - Duration::from_secs(80));
        assert_eq!(
            Cooldown::default()
                .arm(&unavailable(Some(&past)), now)
                .delay
                .as_secs(),
            15
        );
    }
    #[test]
    fn fake_time_emits_once_and_cancel_invalidates_callbacks() {
        let now = Instant::now();
        let mut timer = WorkerTimer(Some((1, now + Duration::from_secs(15))));
        assert_eq!(timer.due(now + Duration::from_secs(14)), None);
        assert_eq!(timer.due(now + Duration::from_secs(15)), Some(1));
        assert_eq!(timer.due(now + Duration::from_secs(120)), None);
        let mut cooldown = Cooldown::default();
        let first = cooldown.arm(&unavailable(None), SystemTime::now());
        cooldown.cancel();
        assert!(cooldown.scheduled.is_none());
        let second = cooldown.arm(&unavailable(None), SystemTime::now());
        assert_ne!(first.token, second.token);
        assert_eq!(second.delay.as_secs(), 30); // Manual failure advances normally.
        timer.0 = Some((second.token, now + second.delay));
        timer.0 = None; // Cancel command/shutdown needs no sleeping timer thread.
        assert_eq!(timer.due(now + Duration::from_secs(1000)), None);
    }
}
