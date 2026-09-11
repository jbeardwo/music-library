//! Frontend-independent backend for the music library.

pub mod album_matching;
pub mod application;
pub mod catalog;
pub mod domain;
pub mod filesystem;
pub mod matching;
pub mod playback;
pub mod recording;
pub mod storage;

pub use application::Library;
pub use storage::{Error, Result};
