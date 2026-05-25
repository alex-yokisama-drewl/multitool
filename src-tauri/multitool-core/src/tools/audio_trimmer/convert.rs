//! Audio Trimmer — pure transform (scaffold).
//!
//! Owns the IPC wire types ([`Opts`]) for the trimmer. The actual
//! `trim_and_fade` implementation lands in commit 2; this file is the
//! placeholder so the orchestrator + Tauri shim can compile against the
//! published `Opts` shape.

use serde::{Deserialize, Serialize};

/// User-facing trim options. Mirrors the form fields on the tool view.
///
/// Range bounds are in milliseconds from the start of the source.
/// `end_ms` is clamped to the source duration in the orchestrator;
/// `start_ms >= end_ms` is rejected pre-encode with a `ProcessingFailed`.
/// Fades are in milliseconds too, clamped to half the trim window when
/// `fade_in_ms + fade_out_ms > (end_ms − start_ms)` — a warning rides
/// along on the success event.
///
/// The UI exposes fades as checkboxes that toggle a fixed `1000 ms`
/// default value; the Rust API keeps the millisecond field so unit
/// tests can hit edge cases (`0`, equal-to-window, overlap) without
/// going through the UI clamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Opts {
    pub start_ms: u64,
    pub end_ms: u64,
    pub fade_in_ms: u32,
    pub fade_out_ms: u32,
}
