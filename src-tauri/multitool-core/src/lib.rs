//! Pure-logic core for multitool.
//!
//! Everything here is testable without spinning up Tauri (SPEC §5.1, §8.1).
//! The Tauri shell in `src-tauri/` depends on this crate and adds command
//! plumbing on top.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod error;
pub mod ipc;

pub use error::{AppError, AppResult};
