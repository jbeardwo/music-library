//! Application-owned catalog data. Providers translate into these values.
use crate::domain::ExternalIdentity;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct CatalogError(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Credit {
    /// Optional strong provider Artist identity, separate from credited presentation.
    pub identity: Option<ExternalIdentity>,
    pub name: String,
    pub join_phrase: String,
}
pub fn credit_display(credits: &[Credit]) -> String {
    credits
        .iter()
        .map(|c| format!("{}{}", c.name, c.join_phrase))
        .collect()
}
#[derive(Clone, Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_offset: Option<u32>,
}
#[derive(Clone, Debug)]
pub struct AlbumCandidate {
    pub credits: Vec<Credit>,
    pub identity: ExternalIdentity,
    pub title: String,
    pub artist: String,
    pub date: String,
    pub primary_type: String,
    pub secondary_types: Vec<String>,
    pub comment: String,
    pub score: Option<u32>,
}
#[derive(Clone, Debug)]
pub struct ReleaseCandidate {
    pub identity: ExternalIdentity,
    pub title: String,
    pub artist: String,
    pub date: String,
    pub country: String,
    pub status: String,
    pub comment: String,
    pub barcode: String,
    pub labels: Vec<String>,
    pub formats: Vec<String>,
    pub disc_count: usize,
    pub track_count: u32,
}
#[derive(Clone, Debug)]
pub struct Release {
    pub album: Album,
    /// Identity used for explicit re-add, not semantic matching.
    pub identity: ExternalIdentity,
    pub identities: Vec<ExternalIdentity>,
    pub title: String,
    pub date: String,
    pub credits: Vec<Credit>,
    pub media: Vec<Medium>,
}
#[derive(Clone, Debug)]
pub struct Medium {
    pub position: u32,
    pub tracks: Vec<Track>,
}
#[derive(Clone, Debug)]
pub struct Track {
    pub position: u32,
    pub title: String,
    pub credits: Vec<Credit>,
    pub identities: Vec<ExternalIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistCandidate {
    pub identity: ExternalIdentity,
    pub name: String,
    pub comment: String,
    pub country: String,
    pub artist_type: String,
    pub score: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistAlbumCandidate {
    pub identity: ExternalIdentity,
    pub title: String,
    pub artist_ids: Vec<ExternalIdentity>,
    pub comment: String,
    pub date: String,
}

/// Calls may block. UI callers must dispatch them off their owning thread.
pub trait CatalogProvider: Send {
    fn search_artists(&mut self, _name: &str) -> Result<Page<ArtistCandidate>, CatalogError> {
        Err(CatalogError(
            "Artist discovery is not supported by this provider".into(),
        ))
    }
    /// Must constrain discovery to the supplied Artist identity, not its display name.
    fn artist_albums(
        &mut self,
        _artist: &ExternalIdentity,
        _title: &str,
    ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
        Err(CatalogError(
            "Artist-scoped Album discovery is not supported by this provider".into(),
        ))
    }
    fn search_albums(
        &mut self,
        query: &str,
        offset: u32,
    ) -> Result<Page<AlbumCandidate>, CatalogError>;
    fn releases(
        &mut self,
        group: &ExternalIdentity,
        offset: u32,
    ) -> Result<Page<ReleaseCandidate>, CatalogError>;
    /// A bounded candidate page for default selection, without rich edition-display data.
    /// Providers may prefer Official candidates, falling back when none exist.
    fn representative_releases(
        &mut self,
        group: &ExternalIdentity,
    ) -> Result<Page<ReleaseCandidate>, CatalogError> {
        self.releases(group, 0)
    }
    fn release(&mut self, identity: &ExternalIdentity) -> Result<Release, CatalogError>;
}

/// Initial friendly Album metadata, independent of the selected edition.
#[derive(Clone, Debug)]
pub struct Album {
    pub identity: ExternalIdentity,
    pub title: String,
    pub date: String,
    pub credits: Vec<Credit>,
}
impl AlbumCandidate {
    pub fn album(&self) -> Album {
        Album {
            identity: self.identity.clone(),
            title: self.title.clone(),
            date: self.date.clone(),
            credits: self.credits.clone(),
        }
    }
}

/// Provisional representative of the supplied bounded candidate set, never canonical.
pub fn representative_release<'a>(
    album: &AlbumCandidate,
    candidates: &'a [ReleaseCandidate],
) -> Option<&'a ReleaseCandidate> {
    let year = |date: &str| date.get(..4).and_then(|s| s.parse::<i32>().ok());
    candidates
        .iter()
        .filter(|r| r.disc_count > 0 && r.track_count > 0)
        .min_by_key(|r| {
            let distance = year(&album.date)
                .zip(year(&r.date))
                .map(|(a, b)| a.abs_diff(b))
                .unwrap_or(u32::MAX);
            let description = format!("{} {}", r.title, r.comment).to_lowercase();
            let expanded = ["deluxe", "expanded", "anniversary", "bonus"]
                .iter()
                .any(|word| description.contains(word));
            (
                !r.status.eq_ignore_ascii_case("official"),
                distance,
                expanded,
                r.disc_count,
                &r.date,
                &r.identity.external_id,
            )
        })
}

/// Small, bounded interaction cache. No network calls occur merely on construction/search.
pub struct CatalogSession<P> {
    provider: P,
    editions: std::collections::VecDeque<(ExternalIdentity, u32, Page<ReleaseCandidate>)>,
    representative: Option<(ExternalIdentity, Page<ReleaseCandidate>)>,
    detail: Option<Release>,
}
impl<P: CatalogProvider> CatalogSession<P> {
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            editions: Default::default(),
            representative: None,
            detail: None,
        }
    }
    pub fn search_albums(
        &mut self,
        query: &str,
        offset: u32,
    ) -> Result<Page<AlbumCandidate>, CatalogError> {
        self.provider.search_albums(query, offset)
    }
    pub fn editions(
        &mut self,
        album: &AlbumCandidate,
        offset: u32,
    ) -> Result<Page<ReleaseCandidate>, CatalogError> {
        if let Some((_, _, page)) = self
            .editions
            .iter()
            .find(|(id, o, _)| *id == album.identity && *o == offset)
        {
            Timing::event("cache=edition_page hit=true");
            return Ok(page.clone());
        }
        let page = self.provider.releases(&album.identity, offset)?;
        if self.editions.len() == 4 {
            self.editions.pop_front();
        }
        self.editions
            .push_back((album.identity.clone(), offset, page.clone()));
        Ok(page)
    }
    /// Rank the first bounded edition page; further editions remain an explicit advanced action.
    pub fn add_album(&mut self, album: &AlbumCandidate) -> Result<Release, CatalogError> {
        let page = match &self.representative {
            Some((id, page)) if *id == album.identity => {
                Timing::event("cache=representative_page hit=true");
                page.clone()
            }
            _ => {
                // A complete, all-Official rich page is also a complete default
                // candidate set. An incomplete unfiltered page is not: filtering
                // at the provider may bring other Official editions onto page one.
                let cached = self.editions.iter().find(|(id, offset, page)| {
                    *id == album.identity
                        && *offset == 0
                        && page.next_offset.is_none()
                        && !page.items.is_empty()
                        && page
                            .items
                            .iter()
                            .all(|r| r.status.eq_ignore_ascii_case("official"))
                });
                let page = match cached {
                    Some((_, _, page)) => {
                        Timing::event("cache=compatible_edition_page hit=true");
                        page.clone()
                    }
                    None => self.provider.representative_releases(&album.identity)?,
                };
                self.representative = Some((album.identity.clone(), page.clone()));
                page
            }
        };
        let selection = Timing::new("representative_selection");
        let id = representative_release(album, &page.items)
            .ok_or_else(||CatalogError("No representative edition with usable track/media information; inspect Editions".into()))?.identity.clone();
        drop(selection);
        self.edition(album, &id)
    }
    pub fn edition(
        &mut self,
        album: &AlbumCandidate,
        id: &ExternalIdentity,
    ) -> Result<Release, CatalogError> {
        let mut release = match &self.detail {
            Some(r) if r.identity == *id => {
                Timing::event("cache=release_detail hit=true");
                r.clone()
            }
            _ => {
                let r = self.provider.release(id)?;
                self.detail = Some(r.clone());
                r
            }
        };
        if release.identity != *id || release.album.identity != album.identity {
            return Err(CatalogError(
                "Selected edition does not belong to this Album".into(),
            ));
        }
        // Friendly search metadata must not be replaced with an edition's title/credit/year.
        release.album = album.album();
        Ok(release)
    }
}

/// Opt-in diagnostic spans for the catalog latency audit; no events unless enabled.
#[doc(hidden)]
pub struct Timing {
    label: String,
    start: Option<std::time::Instant>,
}
impl Timing {
    pub fn enabled() -> bool {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var_os("MUSIC_LIBRARY_CATALOG_TIMING").is_some())
    }
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            start: Self::enabled().then(std::time::Instant::now),
        }
    }
    pub fn detail(label: impl Into<String>) -> Self {
        static DETAIL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let enabled = Self::enabled()
            && *DETAIL
                .get_or_init(|| std::env::var_os("MUSIC_LIBRARY_CATALOG_TIMING_DETAIL").is_some());
        Self {
            label: label.into(),
            start: enabled.then(std::time::Instant::now),
        }
    }
    pub fn event(message: impl std::fmt::Display) {
        if Self::enabled() {
            eprintln!("catalog-timing {message}");
        }
    }
}
impl Drop for Timing {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            eprintln!(
                "catalog-timing phase={} ms={:.3}",
                self.label,
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
}
