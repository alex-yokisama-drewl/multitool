//! Filesystem path helpers shared across tools.
//!
//! Implements the duplicate-name policy from ARCHITECTURE §3.3: never overwrite
//! silently — append ` (1)`, ` (2)`, … until a free name is found.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Return a path that does not currently exist on disk, derived from `target`.
///
/// If `target` is free, returns it unchanged. Otherwise appends ` (1)`,
/// ` (2)`, … to the file stem (when an extension is present) or to the full
/// file name (when it isn't), and retries until the first free name is found.
///
/// Examples (per ARCHITECTURE §3.3):
/// - `foo.txt` exists → `foo (1).txt`
/// - `foo.tar.gz` exists → `foo.tar (1).gz` (suffix lands before the final
///   extension, matching Finder / Windows Explorer)
/// - `Makefile` exists → `Makefile (1)`
/// - `report_pages/` exists → `report_pages (1)`
///
/// **Not race-safe by design.** Another process could claim the returned path
/// between this check and any subsequent create. Single-threaded use only.
pub fn unique_path(target: &Path) -> std::io::Result<PathBuf> {
    if !target.try_exists()? {
        return Ok(target.to_path_buf());
    }

    let parent = target.parent();
    let extension = target.extension();
    let base: OsString = if extension.is_some() {
        target
            .file_stem()
            .map(ToOwned::to_owned)
            .unwrap_or_default()
    } else {
        target
            .file_name()
            .map(ToOwned::to_owned)
            .unwrap_or_default()
    };

    for n in 1u32..=u32::MAX {
        let mut candidate_name = base.clone();
        candidate_name.push(format!(" ({n})"));
        if let Some(ext) = extension {
            candidate_name.push(".");
            candidate_name.push(ext);
        }
        let candidate = parent
            .map(|p| p.join(&candidate_name))
            .unwrap_or_else(|| PathBuf::from(&candidate_name));
        if !candidate.try_exists()? {
            return Ok(candidate);
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "unique_path: exhausted u32 suffixes",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn returns_unchanged_when_target_is_free() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("missing.txt");

        let got = unique_path(&target).unwrap();

        assert_eq!(got, target);
    }

    #[test]
    fn appends_one_when_file_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("report.txt");
        fs::write(&target, b"").unwrap();

        let got = unique_path(&target).unwrap();

        assert_eq!(got, tmp.path().join("report (1).txt"));
    }

    #[test]
    fn increments_until_free() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("report.txt");
        fs::write(&target, b"").unwrap();
        fs::write(tmp.path().join("report (1).txt"), b"").unwrap();

        let got = unique_path(&target).unwrap();

        assert_eq!(got, tmp.path().join("report (2).txt"));
    }

    #[test]
    fn appends_one_when_directory_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("report_pages");
        fs::create_dir(&target).unwrap();

        let got = unique_path(&target).unwrap();

        assert_eq!(got, tmp.path().join("report_pages (1)"));
    }

    // Decision: with `foo.tar.gz`, `Path::extension()` returns `gz` and
    // `file_stem()` returns `foo.tar`, so the suffix lands before the final
    // `.gz`. This matches macOS Finder and Windows Explorer; treating `.tar.gz`
    // as a single compound extension would require a per-format list we don't
    // want to maintain.
    #[test]
    fn multi_dot_stem_suffix_lands_before_final_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("archive.tar.gz");
        fs::write(&target, b"").unwrap();

        let got = unique_path(&target).unwrap();

        assert_eq!(got, tmp.path().join("archive.tar (1).gz"));
    }

    #[test]
    fn no_extension_suffix_appended_to_full_name() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("Makefile");
        fs::write(&target, b"").unwrap();

        let got = unique_path(&target).unwrap();

        assert_eq!(got, tmp.path().join("Makefile (1)"));
    }
}
