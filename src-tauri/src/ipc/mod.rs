//! IPC plumbing: command/event wiring shared by every tool.
//!
//! The job registry maps a UI-generated [`JobId`] to a [`CancellationToken`]
//! so long-running commands can be cooperatively cancelled. A tool's command
//! handler registers a token before spawning work and unregisters it on
//! completion; the webview cancels via the `cancel_job` command.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct JobId(pub String);

#[derive(Default)]
pub struct JobRegistry {
    jobs: Mutex<HashMap<JobId, CancellationToken>>,
}

impl JobRegistry {
    pub fn register(&self, id: JobId) -> CancellationToken {
        let token = CancellationToken::new();
        let mut jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        jobs.insert(id, token.clone());
        token
    }

    pub fn unregister(&self, id: &JobId) {
        let mut jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        jobs.remove(id);
    }

    pub fn cancel(&self, id: &JobId) -> bool {
        let mut jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        match jobs.remove(id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }
}

#[tauri::command]
pub fn cancel_job(job_id: JobId, registry: tauri::State<'_, JobRegistry>) -> bool {
    registry.cancel(&job_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_signals_registered_token() {
        let registry = JobRegistry::default();
        let id = JobId("job-1".into());
        let token = registry.register(id.clone());

        assert!(!token.is_cancelled());
        assert!(registry.cancel(&id));
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancel_unknown_id_is_noop() {
        let registry = JobRegistry::default();
        assert!(!registry.cancel(&JobId("missing".into())));
    }

    #[test]
    fn unregister_drops_token_without_cancelling() {
        let registry = JobRegistry::default();
        let id = JobId("job-2".into());
        let token = registry.register(id.clone());

        registry.unregister(&id);
        assert!(!token.is_cancelled());
        assert!(!registry.cancel(&id));
    }
}
