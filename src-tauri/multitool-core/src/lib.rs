//! Pure-logic core for multitool.
//!
//! Everything here is testable without spinning up Tauri (ARCHITECTURE §3.1, §4).
//! The Tauri shell in `src-tauri/` depends on this crate and adds command
//! plumbing on top.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod audio_codecs;
pub mod error;
pub mod fs;
pub mod image;
pub mod ipc;
pub mod pdfium;
pub mod tools;

pub use error::{AppError, AppResult};
