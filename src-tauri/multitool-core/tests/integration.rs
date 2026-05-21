//! Crate-level integration smoke tests.
//!
//! These exercise `multitool_core`'s public surface from outside the crate
//! (the same way the Tauri shell does), so they catch visibility / module
//! re-export regressions that a `#[cfg(test)]` inner test would miss.

use multitool_core::{ipc::JobId, AppError};

#[test]
fn app_error_serializes_via_public_surface() {
    let err = AppError::ProcessingFailed {
        detail: "boom".into(),
    };
    let value = serde_json::to_value(&err).expect("serialization succeeds");
    assert_eq!(value["kind"], "ProcessingFailed");
    assert_eq!(value["message"], "processing failed: boom");
}

#[test]
fn job_registry_round_trips_through_public_api() {
    let registry = multitool_core::ipc::JobRegistry::default();
    let id = JobId("integration".into());

    let token = registry.register(id.clone());
    assert!(registry.cancel(&id));
    assert!(token.is_cancelled());
}
