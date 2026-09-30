//! Pane-local action inputs; only this layer expands containers into canonical Tracks.
use crate::{
    Result,
    browse::{Pane, QueueReader, Request},
    domain::TrackSearchResult,
    playlist::read_selected_entries,
};
#[derive(Clone, Debug)]
pub enum Target {
    Artists(Vec<String>),
    Genres(Vec<String>),
    Albums(Vec<String>),
    Songs(Vec<String>),
    Playlists(Vec<String>),
    PlaylistEntries {
        playlists: Vec<String>,
        entries: Vec<String>,
    },
}
impl QueueReader {
    pub fn resolve(&self, target: &Target, ordering: &Request) -> Result<Vec<TrackSearchResult>> {
        let empty = match target {
            Target::Artists(ids)
            | Target::Genres(ids)
            | Target::Albums(ids)
            | Target::Songs(ids)
            | Target::Playlists(ids) => ids.is_empty(),
            Target::PlaylistEntries { entries, .. } => entries.is_empty(),
        };
        if empty {
            return Ok(vec![]);
        }
        let mut request = Request {
            pane: Pane::Songs,
            sort: ordering.sort,
            album_sort: ordering.album_sort,
            ..Default::default()
        };
        match target {
            Target::Artists(ids) => request.artists = ids.clone(),
            Target::Genres(ids) => request.genres = ids.clone(),
            Target::Albums(ids) => request.albums = ids.clone(),
            Target::Songs(ids) => request.tracks = ids.clone(),
            Target::Playlists(ids) => {
                return Ok(read_selected_entries(&self.0, ids, None, None, &[])?
                    .into_iter()
                    .filter_map(|r| r.track)
                    .collect());
            }
            Target::PlaylistEntries { playlists, entries } => {
                return Ok(
                    read_selected_entries(&self.0, playlists, None, None, entries)?
                        .into_iter()
                        .filter_map(|r| r.track)
                        .collect(),
                );
            }
        }
        self.read_request(&request)
    }
}
