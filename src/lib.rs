//! Frontend-independent backend for the music library.

pub mod application;
pub mod domain;
pub mod filesystem;
pub mod storage;

pub use application::Library;
pub use storage::{Error, Result};
