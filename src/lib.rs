//! Frontend-independent backend for the music library.

pub mod album_candidates;
pub mod album_matching;
pub mod album_program;
pub mod application;
pub mod artist_credit;
mod artist_identity;
pub mod artwork;
pub mod browse;
pub mod catalog;
pub mod catalog_date;
pub mod catalog_search;
pub mod domain;
pub mod edition;
pub mod edition_storage;
pub mod filesystem;
pub mod library_removal;
pub mod library_search;
pub mod local_ingestion;
pub mod manual_track;
pub mod matching;
pub mod output;
pub mod playback;
pub mod playback_resolver;
pub mod playlist;
pub mod provenance;
pub mod provenance_acceptance;
mod provenance_storage;
pub mod provider_chain;
pub mod recording;
pub mod song_resolution;
pub mod storage;

pub use application::Library;
pub use storage::{Error, Result};
