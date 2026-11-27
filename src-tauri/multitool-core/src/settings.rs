//! Durable user settings: a versioned JSON document on disk.
//!
//! Pure logic — the Tauri shell resolves the app-config path and owns the
//! commands (`src-tauri/src/settings/`); everything worth testing lives here.
//!
//! **The document is an opaque string-keyed blob.** This crate validates the
//! envelope (version + object shape) and nothing inside it. Every schema is
//! owned by the frontend, which sanitizes each value against its own defaults
//! on read. That is deliberate: it keeps the registry contract from
//! ARCHITECTURE §3.1 intact, because adding a tool never touches Rust settings
//! code.
//!
//! **Loading never fails.** A missing, unreadable, corrupt, or
//! foreign-version document resolves to defaults — losing preferences is an
//! annoyance, refusing to start is a bug. [`LoadOutcome`] tells the caller
//! which happened so it can log.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{AppError, AppResult};

/// Schema version of the settings document.
///
/// Bump only for a change the frontend's per-key sanitizers cannot absorb.
/// A document carrying any other version loads as defaults (see
/// [`LoadOutcome::Recovered`]) — field-level drift is the sanitizers' job,
/// not this number's.
pub const SETTINGS_VERSION: u32 = 1;

/// Refuse to parse anything larger than this. The document holds a few dozen
/// short scalars; a file this big means corruption or something else writing
/// to the path, and either way parsing it is not worth the memory.
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsDoc {
    pub version: u32,
    /// Opaque frontend-owned values. `serde(default)` so a document written
    /// by a build that had no entries yet still parses.
    #[serde(default)]
    pub entries: Map<String, Value>,
}

impl Default for SettingsDoc {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            entries: Map::new(),
        }
    }
}

/// What [`load`] actually found, so the caller can log it. Every variant
/// still yields a usable document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadOutcome {
    /// Parsed a document at the current version.
    Loaded,
    /// No file yet — first run, or the user deleted it.
    Missing,
    /// Something was there but unusable; defaults were substituted. The
    /// string is human-readable and meant for a log line, not the UI.
    Recovered(String),
}

/// Read the settings document at `path`, falling back to defaults.
///
/// Never returns an error: see the module docs.
pub fn load(path: &Path) -> (SettingsDoc, LoadOutcome) {
    let metadata = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return (SettingsDoc::default(), LoadOutcome::Missing);
        }
        Err(err) => {
            return (
                SettingsDoc::default(),
                LoadOutcome::Recovered(format!("stat failed: {err}")),
            );
        }
    };

    if metadata.len() > MAX_BYTES {
        return (
            SettingsDoc::default(),
            LoadOutcome::Recovered(format!(
                "file is {} bytes, over the {MAX_BYTES}-byte ceiling",
                metadata.len()
            )),
        );
    }

    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) => {
            return (
                SettingsDoc::default(),
                LoadOutcome::Recovered(format!("read failed: {err}")),
            );
        }
    };

    let doc: SettingsDoc = match serde_json::from_str(&raw) {
        Ok(doc) => doc,
        Err(err) => {
            return (
                SettingsDoc::default(),
                LoadOutcome::Recovered(format!("malformed JSON: {err}")),
            );
        }
    };

    if doc.version != SETTINGS_VERSION {
        return (
            SettingsDoc::default(),
            LoadOutcome::Recovered(format!(
                "version {} is not the supported {SETTINGS_VERSION}",
                doc.version
            )),
        );
    }

    (doc, LoadOutcome::Loaded)
}

/// Write `doc` to `path`, creating parent directories as needed.
///
/// Atomic: serializes to a sibling temp file, flushes it, then renames over
/// the target. A crash mid-write leaves the previous document intact rather
/// than a truncated one — settings are written on a debounce while the user
/// is still clicking, so a half-written file is a realistic failure, not a
/// theoretical one.
pub fn save(path: &Path, doc: &SettingsDoc) -> AppResult<()> {
    let parent = path.parent().ok_or_else(|| AppError::ProcessingFailed {
        detail: format!("settings path has no parent directory: {}", path.display()),
    })?;
    fs::create_dir_all(parent).map_err(|err| io_err(parent, "create settings directory", &err))?;

    let serialized = serde_json::to_vec_pretty(doc).map_err(|err| AppError::ProcessingFailed {
        detail: format!("serialize settings: {err}"),
    })?;

    let temp = temp_path(path);
    // Scoped so the handle is closed before the rename — Windows refuses to
    // rename a file that still has an open handle.
    {
        let mut file =
            fs::File::create(&temp).map_err(|err| io_err(&temp, "create settings temp", &err))?;
        file.write_all(&serialized)
            .map_err(|err| io_err(&temp, "write settings temp", &err))?;
        file.flush()
            .map_err(|err| io_err(&temp, "flush settings temp", &err))?;
    }

    if let Err(err) = fs::rename(&temp, path) {
        // Best-effort cleanup; the rename failure is the error worth
        // surfacing, so a failed removal is deliberately swallowed.
        let _ = fs::remove_file(&temp);
        return Err(io_err(path, "replace settings file", &err));
    }

    Ok(())
}

/// Sibling temp path for the atomic write. Appends rather than replacing the
/// extension so `settings.json` → `settings.json.tmp` (and never collides
/// with the real document).
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

fn io_err(path: &Path, action: &str, err: &std::io::Error) -> AppError {
    match err.kind() {
        std::io::ErrorKind::PermissionDenied => AppError::PermissionDenied {
            path: path.display().to_string(),
        },
        _ => AppError::ProcessingFailed {
            detail: format!("{action} ({}): {err}", path.display()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc_with(key: &str, value: Value) -> SettingsDoc {
        let mut doc = SettingsDoc::default();
        doc.entries.insert(key.to_string(), value);
        doc
    }

    #[test]
    fn missing_file_loads_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let (doc, outcome) = load(&dir.path().join("settings.json"));
        assert_eq!(doc, SettingsDoc::default());
        assert_eq!(outcome, LoadOutcome::Missing);
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let written = doc_with(
            "tool:pdf-to-images",
            json!({ "dpi": 300, "format": "jpeg" }),
        );

        save(&path, &written).unwrap();
        let (read, outcome) = load(&path);

        assert_eq!(read, written);
        assert_eq!(outcome, LoadOutcome::Loaded);
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/deeper/settings.json");

        save(&path, &doc_with("k", json!(1))).unwrap();

        assert!(path.exists());
        assert_eq!(load(&path).1, LoadOutcome::Loaded);
    }

    #[test]
    fn save_overwrites_an_existing_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        save(&path, &doc_with("k", json!("first"))).unwrap();
        save(&path, &doc_with("k", json!("second"))).unwrap();

        assert_eq!(load(&path).0.entries["k"], json!("second"));
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        save(&path, &SettingsDoc::default()).unwrap();

        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "stray temp files: {leftovers:?}");
    }

    #[test]
    fn malformed_json_loads_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{ not json at all").unwrap();

        let (doc, outcome) = load(&path);

        assert_eq!(doc, SettingsDoc::default());
        assert!(matches!(outcome, LoadOutcome::Recovered(ref r) if r.contains("malformed")));
    }

    #[test]
    fn foreign_version_loads_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(
            &path,
            json!({ "version": SETTINGS_VERSION + 1, "entries": { "k": 1 } }).to_string(),
        )
        .unwrap();

        let (doc, outcome) = load(&path);

        assert_eq!(doc, SettingsDoc::default());
        assert!(matches!(outcome, LoadOutcome::Recovered(ref r) if r.contains("version")));
    }

    #[test]
    fn document_without_entries_key_parses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, json!({ "version": SETTINGS_VERSION }).to_string()).unwrap();

        let (doc, outcome) = load(&path);

        assert_eq!(outcome, LoadOutcome::Loaded);
        assert!(doc.entries.is_empty());
    }

    #[test]
    fn oversized_file_loads_defaults_without_parsing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let padding = "x".repeat(MAX_BYTES as usize + 1);
        fs::write(
            &path,
            json!({ "version": SETTINGS_VERSION, "entries": { "k": padding } }).to_string(),
        )
        .unwrap();

        let (doc, outcome) = load(&path);

        assert_eq!(doc, SettingsDoc::default());
        assert!(matches!(outcome, LoadOutcome::Recovered(ref r) if r.contains("ceiling")));
    }

    #[test]
    fn entry_values_survive_as_written() {
        // The blob is opaque: nested objects, arrays, nulls and floats all
        // round-trip untouched, because only the frontend knows what they mean.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let value = json!({
            "nested": { "a": [1, 2.5, null, true] },
            "unicode": "café / 日本語",
        });

        save(&path, &doc_with("k", value.clone())).unwrap();

        assert_eq!(load(&path).0.entries["k"], value);
    }

    #[test]
    fn temp_path_appends_rather_than_replacing_the_extension() {
        assert_eq!(
            temp_path(Path::new("/cfg/settings.json")),
            PathBuf::from("/cfg/settings.json.tmp")
        );
    }

    #[test]
    fn save_reports_a_typed_error_when_the_path_is_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::create_dir(&path).unwrap();

        let err = save(&path, &SettingsDoc::default()).unwrap_err();

        assert!(matches!(
            err,
            AppError::ProcessingFailed { .. } | AppError::PermissionDenied { .. }
        ));
    }
}
