//! Ordered fallback, not cross-provider enrichment. Only the active Album job
//! grants a provider worker permission to start work. No HTTP or SQLite ownership.
use crate::{
    Library,
    album_matching::{AlbumMatcher, AutoMatchPolicy, CircuitState, MatchOutcome, MatchReply},
    album_program,
    catalog::{ArtistAlbumCandidate, MatchingScope},
    domain::{AlbumId, ImportedRelease},
    storage::{Error, Result},
};
use std::collections::{HashMap, VecDeque};

pub struct ProviderSlot {
    pub scope: MatchingScope,
    pub matcher: std::result::Result<AlbumMatcher, String>,
}
#[derive(Clone, Debug)]
pub struct Attempt {
    pub provider: String,
    pub outcome: MatchOutcome,
}
struct Slot {
    scope: MatchingScope,
    matcher: Option<AlbumMatcher>,
    configuration: Option<String>,
    ready: bool,
}
struct Job {
    album: AlbumId,
    order: Vec<usize>,
    at: usize,
    deferred: Vec<usize>,
    pinned: bool,
    resolved: bool,
}
pub struct ProviderChain {
    slots: Vec<Slot>,
    legacy: bool,
    queue: VecDeque<Job>,
    waiting: VecDeque<Job>,
    active: Option<Job>,
    selected: HashMap<AlbumId, usize>,
    outcomes: HashMap<AlbumId, MatchOutcome>,
    history: HashMap<AlbumId, Vec<Attempt>>,
}
impl From<AlbumMatcher> for ProviderChain {
    fn from(m: AlbumMatcher) -> Self {
        let scope = m.scope().clone();
        Self {
            slots: vec![Slot {
                scope,
                matcher: Some(m),
                configuration: None,
                ready: false,
            }],
            legacy: true,
            queue: VecDeque::new(),
            waiting: VecDeque::new(),
            active: None,
            selected: HashMap::new(),
            outcomes: HashMap::new(),
            history: HashMap::new(),
        }
    }
}
impl ProviderChain {
    pub fn new(providers: Vec<ProviderSlot>) -> Result<Self> {
        let mut names = std::collections::HashSet::new();
        if providers.is_empty()
            || providers
                .iter()
                .any(|p| p.matcher.as_ref().is_ok_and(|m| m.scope() != &p.scope))
            || providers
                .iter()
                .any(|p| !names.insert(p.scope.provider.clone()))
        {
            return Err(Error::Invalid(
                "Configure a nonempty chain of distinct providers".into(),
            ));
        }
        if providers.len() == 1 && providers[0].matcher.is_ok() {
            return Ok(providers
                .into_iter()
                .next()
                .unwrap()
                .matcher
                .unwrap()
                .into());
        }
        Ok(Self {
            slots: providers
                .into_iter()
                .map(|p| match p.matcher {
                    Ok(m) => Slot {
                        scope: p.scope,
                        matcher: Some(m),
                        configuration: None,
                        ready: false,
                    },
                    Err(e) => Slot {
                        scope: p.scope,
                        matcher: None,
                        configuration: Some(e),
                        ready: false,
                    },
                })
                .collect(),
            legacy: false,
            queue: VecDeque::new(),
            waiting: VecDeque::new(),
            active: None,
            selected: HashMap::new(),
            outcomes: HashMap::new(),
            history: HashMap::new(),
        })
    }
    fn first(&self) -> &AlbumMatcher {
        self.slots[0].matcher.as_ref().unwrap()
    }
    fn first_mut(&mut self) -> &mut AlbumMatcher {
        self.slots[0].matcher.as_mut().unwrap()
    }
    fn index(&self, id: &AlbumId) -> usize {
        *self.selected.get(id).unwrap_or(&0)
    }
    fn matcher(&self, id: &AlbumId) -> Option<&AlbumMatcher> {
        self.slots[self.index(id)].matcher.as_ref()
    }
    pub fn provider(&self, id: &AlbumId) -> &str {
        &self.slots[self.index(id)].scope.provider
    }
    pub fn provider_messages(&self) -> Vec<String> {
        self.slots
            .iter()
            .filter_map(|s| {
                if let Some(e) = &s.configuration {
                    Some(format!("{} configuration: {e}", s.scope.provider))
                } else if let Some(m) = &s.matcher
                    && let CircuitState::Unavailable(e) = m.circuit_state()
                {
                    Some(format!(
                        "{} unavailable; retrying preserved work when due: {e}",
                        s.scope.provider
                    ))
                } else {
                    None
                }
            })
            .collect()
    }
    pub fn attempts(&self, id: &AlbumId) -> &[Attempt] {
        self.history.get(id).map_or(&[], Vec::as_slice)
    }
    fn record(&mut self, id: &AlbumId, i: usize, outcome: MatchOutcome) {
        if let MatchOutcome::ConfigurationError(error) = &outcome {
            self.slots[i].configuration = Some(error.clone());
        }
        let provider = self.slots[i].scope.provider.clone();
        let history = self.history.entry(id.clone()).or_default();
        if let Some(a) = history.iter_mut().find(|a| a.provider == provider) {
            a.outcome = outcome.clone()
        } else {
            history.push(Attempt {
                provider,
                outcome: outcome.clone(),
            })
        }
        self.selected.insert(id.clone(), i);
        self.outcomes.insert(id.clone(), outcome);
    }
    fn job(&self, library: &Library, id: &AlbumId) -> Result<Job> {
        let identities = library.list_album_external_identities(id)?;
        let manual = library.manual_track_associations(id)?;
        // Manual provider choices take precedence; otherwise the first configured
        // provider with an accepted Album identity wins before any rediscovery.
        let manual_provider = self.slots.iter().position(|s| {
            manual.iter().any(|m| m.album.provider == s.scope.provider)
                || s.matcher.as_ref().is_some_and(|m| m.has_manual_artist(id))
        });
        let known = manual_provider.or_else(|| {
            self.slots.iter().position(|s| {
                identities
                    .iter()
                    .any(|e| e.provider == s.scope.provider && e.kind == s.scope.album_kind)
            })
        });
        Ok(Job {
            album: id.clone(),
            order: known.map_or_else(|| (0..self.slots.len()).collect(), |i| vec![i]),
            at: 0,
            deferred: vec![],
            pinned: known.is_some(),
            resolved: known.is_some_and(|i| {
                identities.iter().any(|e| {
                    e.provider == self.slots[i].scope.provider
                        && e.kind == self.slots[i].scope.album_kind
                })
            }),
        })
    }
    pub fn after_import(
        &mut self,
        library: &Library,
        imports: &[ImportedRelease],
        policy: AutoMatchPolicy,
    ) -> Result<Vec<(AlbumId, MatchOutcome)>> {
        if self.legacy {
            return self.first_mut().after_import(library, imports, policy);
        }
        if !policy.enabled {
            return Ok(vec![]);
        }
        let mut updates = vec![];
        for import in imports {
            let id = library.album_for_release(&import.release_id)?.album_id;
            if !self.outcomes.contains_key(&id) {
                self.match_album(library, &id)?;
            }
            updates.push((
                id.clone(),
                self.outcome(&id).cloned().unwrap_or(MatchOutcome::Pending),
            ));
        }
        Ok(updates)
    }
    pub fn match_album(&mut self, library: &Library, id: &AlbumId) -> Result<MatchOutcome> {
        if self.legacy {
            return self.first_mut().match_album(library, id);
        }
        if self.active.as_ref().is_some_and(|j| &j.album == id)
            || self.queue.iter().any(|j| &j.album == id)
        {
            return Ok(MatchOutcome::Pending);
        }
        self.waiting.retain(|j| &j.album != id);
        self.history.remove(id);
        let job = self.job(library, id)?;
        self.selected.insert(id.clone(), job.order[0]);
        self.queue.push_back(job);
        self.outcomes.insert(id.clone(), MatchOutcome::Pending);
        self.pump(library)?;
        Ok(self.outcome(id).cloned().unwrap_or(MatchOutcome::Pending))
    }
    fn success(outcome: &MatchOutcome) -> bool {
        matches!(
            outcome,
            MatchOutcome::Matched(_) | MatchOutcome::MatchedClose(_) | MatchOutcome::AlreadyMatched
        )
    }
    fn finish_attempt(&mut self, mut job: Job, outcome: MatchOutcome, program: bool) {
        let i = job.order[job.at];
        let id = job.album.clone();
        self.record(&id, i, outcome.clone());
        if matches!(outcome, MatchOutcome::Deferred(_)) {
            self.slots[i].matcher.as_mut().unwrap().yield_album(&id);
            job.deferred.push(i);
            if job.resolved {
                self.outcomes
                    .insert(id.clone(), MatchOutcome::AlreadyMatched);
            }
        } else if Self::success(&outcome)
            || program
            || matches!(outcome, MatchOutcome::Skipped | MatchOutcome::Disabled)
        {
            return;
        }
        job.at += 1;
        self.queue.push_front(job);
    }
    fn pump(&mut self, library: &Library) -> Result<()> {
        if self.active.is_some() {
            return Ok(());
        }
        while let Some(mut job) = {
            if self.queue.is_empty() {
                let ready = self
                    .waiting
                    .iter()
                    .flat_map(|j| j.order.iter())
                    .find(|i| self.slots[**i].ready)
                    .copied();
                if let Some(i) = ready {
                    self.wake(i);
                }
            }
            self.queue.pop_front()
        } {
            if job.at == job.order.len() {
                // Preserve the most useful manual result, not just the last miss.
                let preferred = self
                    .attempts(&job.album)
                    .iter()
                    .find(|a| matches!(a.outcome, MatchOutcome::ArtistAmbiguous(_)))
                    .or_else(|| {
                        self.attempts(&job.album).iter().find(|a| {
                            matches!(
                                a.outcome,
                                MatchOutcome::AlbumEquivalent { .. }
                                    | MatchOutcome::AlbumAmbiguous(_)
                            )
                        })
                    })
                    .cloned();
                if !job.pinned
                    && let Some(a) = preferred
                {
                    let i = self
                        .slots
                        .iter()
                        .position(|s| s.scope.provider == a.provider)
                        .unwrap();
                    self.record(&job.album, i, a.outcome);
                }
                if !job.deferred.is_empty() {
                    job.order = std::mem::take(&mut job.deferred);
                    job.at = 0;
                    self.waiting.push_back(job);
                }
                continue;
            }
            let i = job.order[job.at];
            let id = job.album.clone();
            if let Some(error) = self.slots[i].configuration.clone() {
                self.record(&id, i, MatchOutcome::ConfigurationError(error));
                job.at += 1;
                self.queue.push_front(job);
                continue;
            }
            let unavailable = match self.slots[i].matcher.as_ref().unwrap().circuit_state() {
                CircuitState::Unavailable(e) => Some(e.clone()),
                _ => None,
            };
            if let Some(e) = unavailable
                && !self.slots[i].ready
            {
                self.record(&id, i, MatchOutcome::Deferred(e));
                if job.resolved {
                    self.outcomes
                        .insert(id.clone(), MatchOutcome::AlreadyMatched);
                }
                job.deferred.push(i);
                job.at += 1;
                self.queue.push_front(job);
                continue;
            }
            let probe = std::mem::take(&mut self.slots[i].ready);
            let m = self.slots[i].matcher.as_mut().unwrap();
            let outcome = m.match_album(library, &id)?;
            if probe {
                m.retry_catalog_matching(library)?;
            }
            let active = m.is_processing();
            self.selected.insert(id.clone(), i);
            self.outcomes.insert(
                id,
                if active && !job.resolved {
                    MatchOutcome::Pending
                } else {
                    outcome.clone()
                },
            );
            if active {
                self.active = Some(job);
                return Ok(());
            }
            self.finish_attempt(job, outcome, false);
        }
        Ok(())
    }
    pub fn complete(&mut self, library: &mut Library, reply: MatchReply) -> MatchOutcome {
        if self.legacy {
            return self.first_mut().complete(library, reply);
        }
        let Some(mut job) = self.active.take() else {
            return MatchOutcome::Skipped;
        };
        let id = job.album.clone();
        let i = job.order[job.at];
        let outcome = self.slots[i]
            .matcher
            .as_mut()
            .unwrap()
            .complete(library, reply);
        if !matches!(outcome, MatchOutcome::Deferred(_)) {
            self.wake_all(i);
        }
        self.record(&id, i, outcome.clone());
        if Self::success(&outcome) {
            job.pinned = true;
            job.resolved = true;
            job.order = vec![i];
            job.at = 0;
            job.deferred.clear();
        }
        if self.slots[i].matcher.as_ref().unwrap().is_processing() {
            self.active = Some(job)
        } else {
            self.finish_attempt(job, outcome, false)
        }
        if let Err(e) = self.pump(library) {
            return MatchOutcome::Error(e.to_string());
        }
        self.outcome(&id)
            .cloned()
            .unwrap_or(MatchOutcome::NoConfidentMatch)
    }
    pub fn complete_programs(
        &mut self,
        library: &mut Library,
        reply: album_program::Reply,
    ) -> album_program::Outcome {
        if self.legacy {
            return self.first_mut().complete_programs(library, reply);
        }
        let Some(job) = self.active.take() else {
            return album_program::Outcome::Error("No active provider job".into());
        };
        let i = job.order[job.at];
        let id = job.album.clone();
        let outcome = self.slots[i]
            .matcher
            .as_mut()
            .unwrap()
            .complete_programs(library, reply);
        let matched = self.slots[i]
            .matcher
            .as_ref()
            .unwrap()
            .outcome(&id)
            .cloned()
            .unwrap_or(MatchOutcome::AlreadyMatched);
        if let album_program::Outcome::Deferred(e) = &outcome {
            self.finish_attempt(job, MatchOutcome::Deferred(e.clone()), true);
            self.outcomes.insert(id.clone(), matched);
        } else {
            self.wake_all(i);
            self.record(&id, i, matched);
        }
        if let Err(e) = self.pump(library) {
            return album_program::Outcome::Error(e.to_string());
        }
        outcome
    }
    pub fn cooldown_elapsed(&mut self, library: &Library, token: u64) -> Result<()> {
        self.cooldown_provider(library, 0, token)
    }
    pub fn cooldown_provider(&mut self, library: &Library, i: usize, token: u64) -> Result<()> {
        if self.legacy {
            return self.first_mut().cooldown_elapsed(library, token);
        }
        if self
            .slots
            .get(i)
            .and_then(|s| s.matcher.as_ref())
            .and_then(AlbumMatcher::retry_schedule)
            .is_some_and(|s| s.token == token)
        {
            self.slots[i].ready = true;
            self.wake(i);
            self.pump(library)?;
        }
        Ok(())
    }
    fn wake(&mut self, i: usize) {
        if let Some(n) = self.waiting.iter().position(|j| j.order.contains(&i)) {
            let mut j = self.waiting.remove(n).unwrap();
            j.order.retain(|p| *p != i);
            j.order.insert(0, i);
            j.at = 0;
            self.queue.push_front(j);
        }
    }
    fn wake_all(&mut self, i: usize) {
        let mut retained = VecDeque::new();
        while let Some(j) = self.waiting.pop_front() {
            if j.order.contains(&i) {
                self.queue.push_back(j)
            } else {
                retained.push_back(j)
            }
        }
        self.waiting = retained;
    }
    pub fn retry_catalog_matching(&mut self, library: &Library) -> Result<()> {
        if self.legacy {
            return self.first_mut().retry_catalog_matching(library);
        }
        for i in 0..self.slots.len() {
            if self.slots[i]
                .matcher
                .as_ref()
                .is_some_and(|m| matches!(m.circuit_state(), CircuitState::Unavailable(_)))
            {
                self.slots[i].ready = true;
            }
        }
        self.pump(library)
    }
    pub fn outcome(&self, id: &AlbumId) -> Option<&MatchOutcome> {
        if self.legacy {
            self.first().outcome(id)
        } else {
            self.outcomes.get(id)
        }
    }
    pub fn matched_album(&self, id: &AlbumId) -> Option<&ArtistAlbumCandidate> {
        self.matcher(id).and_then(|m| m.matched_album(id))
    }
    pub fn program_outcome(&self, id: &AlbumId) -> Option<&album_program::Outcome> {
        self.matcher(id).and_then(|m| m.program_outcome(id))
    }
    pub fn cached_programs(&self, id: &AlbumId) -> Option<&album_program::Programs> {
        self.matcher(id).and_then(|m| m.cached_programs(id))
    }
    pub fn recording_outcome(&self, id: &AlbumId) -> Option<&crate::recording::Outcome> {
        self.matcher(id).and_then(|m| m.recording_outcome(id))
    }
    pub fn pending_count(&self) -> usize {
        if self.legacy {
            self.first().pending_count()
        } else {
            self.queue.len() + self.waiting.len() + usize::from(self.active.is_some())
        }
    }
    pub fn retry_schedule(&self) -> Option<crate::album_matching::RetrySchedule> {
        self.retry_schedule_for(0)
    }
    pub fn retry_schedule_for(&self, i: usize) -> Option<crate::album_matching::RetrySchedule> {
        self.slots
            .get(i)
            .and_then(|s| s.matcher.as_ref())
            .and_then(AlbumMatcher::retry_schedule)
    }
    pub fn is_processing(&self) -> bool {
        if self.legacy {
            self.first().is_processing()
        } else {
            self.active.is_some()
        }
    }
    pub fn probe_pending(&self) -> bool {
        self.slots
            .iter()
            .filter_map(|s| s.matcher.as_ref())
            .any(AlbumMatcher::probe_pending)
    }
    pub fn circuit_state(&self) -> &CircuitState {
        static AVAILABLE: CircuitState = CircuitState::Available;
        if self.legacy {
            return self.first().circuit_state();
        }
        if self.active.is_some() {
            return &AVAILABLE;
        }
        self.slots
            .iter()
            .filter_map(|s| s.matcher.as_ref())
            .map(AlbumMatcher::circuit_state)
            .find(|s| matches!(s, CircuitState::Unavailable(_)))
            .unwrap_or(&AVAILABLE)
    }
    pub fn clear_program_outcome(&mut self, id: &AlbumId) {
        let i = self.index(id);
        if let Some(m) = self.slots[i].matcher.as_mut() {
            m.clear_program_outcome(id)
        }
    }
    pub fn request_album_programs(&mut self, library: &Library, id: &AlbumId) -> Result<()> {
        if self.legacy {
            return self.first_mut().request_album_programs(library, id);
        }
        if self.active.as_ref().is_some_and(|j| &j.album == id)
            || self.queue.iter().any(|j| &j.album == id)
        {
            return Ok(());
        }
        let i = match self.selected.get(id) {
            Some(i) => *i,
            None => self.job(library, id)?.order[0],
        };
        self.selected.insert(id.clone(), i);
        self.waiting.retain(|j| &j.album != id);
        self.queue.push_back(Job {
            album: id.clone(),
            order: vec![i],
            at: 0,
            deferred: vec![],
            pinned: true,
            resolved: true,
        });
        self.pump(library)
    }
    pub fn select_artist(
        &mut self,
        library: &mut Library,
        id: &AlbumId,
        index: usize,
    ) -> Result<MatchOutcome> {
        if self.legacy {
            return self.first_mut().select_artist(library, id, index);
        }
        let i = match self.selected.get(id) {
            Some(i) => *i,
            None => self.job(library, id)?.order[0],
        };
        self.selected.insert(id.clone(), i);
        let outcome = self.slots[i]
            .matcher
            .as_mut()
            .ok_or_else(|| Error::Invalid("Provider configuration unavailable".into()))?
            .select_artist_local(library, id, index)?;
        if outcome != MatchOutcome::NoConfidentMatch {
            return Ok(outcome);
        }
        self.waiting.retain(|j| &j.album != id);
        self.queue.retain(|j| &j.album != id);
        let job = Job {
            album: id.clone(),
            order: vec![i],
            at: 0,
            deferred: vec![],
            pinned: true,
            resolved: false,
        };
        self.queue.push_back(job);
        self.outcomes.insert(id.clone(), MatchOutcome::Pending);
        self.pump(library)?;
        Ok(MatchOutcome::Pending)
    }
    pub fn complete_recordings(
        &mut self,
        library: &mut Library,
        reply: crate::recording::Reply,
    ) -> crate::recording::Outcome {
        self.first_mut().complete_recordings(library, reply)
    }
}
