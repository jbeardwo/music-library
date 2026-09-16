//! Frontend-independent backend for the music library.

pub mod album_candidates;
pub mod album_matching;
pub mod album_program;
pub mod application;
pub mod catalog;
pub mod domain;
pub mod edition;
pub mod edition_storage;
pub mod filesystem;
pub mod manual_track;
pub mod matching;
pub mod playback;
pub mod provenance;
pub mod provenance_acceptance;
mod provenance_storage;
pub mod provider_chain;
pub mod recording;
pub mod storage;

pub use application::Library;
pub use storage::{Error, Result};
