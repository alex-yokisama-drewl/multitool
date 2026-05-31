//! Image resize — single-image geometric transform that preserves the
//! source format.
//!
//! Public surface stays minimal: types + entry points only. See
//! [`convert::resize_one`] for the pure transform; orchestrator + Tauri
//! shim layer on top in later commits.

pub mod convert;

pub use convert::{resize_one, Background, FitMode, Opts};
