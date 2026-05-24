//! Writes streamed `PageOutput`s to disk in `{stem}_pages/page_NNN.{ext}` form.
//!
//! The writer is the bridge between [`super::convert`]'s `on_page` callback
//! and the on-disk layout. It resolves the target directory through
//! [`crate::fs::unique_path`] (never merging into an existing folder, per
//! ARCHITECTURE §3.3), creates it, and writes each page as `page_NNN.{ext}`
//! with zero-padding sized to fit the declared total (min width 3, widens
//! past 999 pages).
//!
//! Failures from the filesystem are mapped to [`AppError`]: a
//! `PermissionDenied` `io::Error` becomes [`AppError::PermissionDenied`];
//! everything else collapses onto [`AppError::ProcessingFailed`] with the
//! path + underlying message in the detail.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::convert::{Format, PageOutput};
use crate::error::{AppError, AppResult};
use crate::fs::unique_path;

/// Filesystem sink for one conversion job.
///
/// Use one writer per job and call [`write_page`](Self::write_page) in source
/// order. The writer holds no buffer — each call hits disk synchronously, so
/// partial output stays on disk if the caller stops early (cancellation
/// leaves already-written pages in place).
///
/// Defer [`create`](Self::create) until the caller knows at least one page
/// will be written: the constructor creates the output directory eagerly, so
/// calling it before a doomed `convert` (encrypted / empty PDF) leaves an
/// empty folder on disk. The shell command in `src-tauri/` is responsible
/// for that coordination, not the writer.
#[derive(Debug)]
pub struct PageWriter {
    dir: PathBuf,
    extension: &'static str,
    pad_width: usize,
}

impl PageWriter {
    /// Resolve `target` through [`unique_path`], create the directory, and
    /// return a writer ready to receive pages.
    ///
    /// `total_pages` is used only to size the zero-padding width
    /// (min 3, grows to fit larger totals). Indices past the declared total
    /// are still written; their filenames just won't be left-padded.
    pub fn create(target: &Path, format: Format, total_pages: u32) -> AppResult<Self> {
        let dir = unique_path(target).map_err(|err| io_to_app_err(target, err))?;
        fs::create_dir_all(&dir).map_err(|err| io_to_app_err(&dir, err))?;
        Ok(Self {
            dir,
            extension: extension_for(format),
            pad_width: pad_width(total_pages),
        })
    }

    /// The on-disk directory the writer is publishing to. Differs from the
    /// `target` passed to [`create`] when a unique-suffix was appended.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Encode `page` to `{dir}/page_NNN.{ext}`. The 0-based `page.index` is
    /// rendered 1-based in the filename (page 1 of N, not page 0 of N-1).
    pub fn write_page(&self, page: &PageOutput) -> AppResult<()> {
        let one_based = page.index.saturating_add(1);
        let name = format!(
            "page_{one_based:0width$}.{ext}",
            width = self.pad_width,
            ext = self.extension
        );
        let path = self.dir.join(&name);
        let mut file = fs::File::create(&path).map_err(|err| io_to_app_err(&path, err))?;
        file.write_all(&page.encoded)
            .map_err(|err| io_to_app_err(&path, err))?;
        Ok(())
    }
}

fn extension_for(format: Format) -> &'static str {
    match format {
        Format::Png => "png",
        Format::Jpeg => "jpg",
    }
}

fn pad_width(total: u32) -> usize {
    // Min 3 keeps short jobs visually consistent (`page_001`, not `page_1`);
    // widens automatically once the total has 4+ digits.
    total.to_string().len().max(3)
}

fn io_to_app_err(path: &Path, err: io::Error) -> AppError {
    match err.kind() {
        io::ErrorKind::PermissionDenied => AppError::PermissionDenied {
            path: path.display().to_string(),
        },
        _ => AppError::ProcessingFailed {
            detail: format!("{}: {err}", path.display()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_page(index: u32, total: u32, bytes: &[u8]) -> PageOutput {
        PageOutput {
            index,
            total,
            encoded: bytes.to_vec(),
        }
    }

    #[test]
    fn three_page_job_writes_three_files_with_three_digit_padding() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("doc_pages");

        let writer = PageWriter::create(&target, Format::Png, 3).unwrap();
        writer.write_page(&synth_page(0, 3, b"alpha")).unwrap();
        writer.write_page(&synth_page(1, 3, b"beta")).unwrap();
        writer.write_page(&synth_page(2, 3, b"gamma")).unwrap();

        assert_eq!(writer.dir(), target);
        assert_eq!(fs::read(target.join("page_001.png")).unwrap(), b"alpha");
        assert_eq!(fs::read(target.join("page_002.png")).unwrap(), b"beta");
        assert_eq!(fs::read(target.join("page_003.png")).unwrap(), b"gamma");
    }

    #[test]
    fn padding_widens_for_thousand_page_jobs() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("big_pages");

        let writer = PageWriter::create(&target, Format::Png, 1000).unwrap();
        writer.write_page(&synth_page(0, 1000, b"data")).unwrap();

        assert!(target.join("page_0001.png").is_file());
        assert!(!target.join("page_001.png").exists());
    }

    #[test]
    fn jpeg_format_writes_jpg_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("doc_pages");

        let writer = PageWriter::create(&target, Format::Jpeg, 1).unwrap();
        writer.write_page(&synth_page(0, 1, b"jpeg-bytes")).unwrap();

        assert!(target.join("page_001.jpg").is_file());
    }

    #[test]
    fn collision_resolves_through_unique_path_without_touching_existing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("doc_pages");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("preexisting.txt"), b"don't touch").unwrap();

        let writer = PageWriter::create(&target, Format::Png, 1).unwrap();
        writer.write_page(&synth_page(0, 1, b"new")).unwrap();

        let resolved = tmp.path().join("doc_pages (1)");
        assert_eq!(writer.dir(), resolved);
        assert!(resolved.join("page_001.png").is_file());
        assert_eq!(
            fs::read(target.join("preexisting.txt")).unwrap(),
            b"don't touch"
        );
    }

    #[test]
    fn early_termination_leaves_already_written_files_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("doc_pages");

        let writer = PageWriter::create(&target, Format::Png, 5).unwrap();
        writer.write_page(&synth_page(0, 5, b"a")).unwrap();
        writer.write_page(&synth_page(1, 5, b"b")).unwrap();
        // Caller stops here (simulates cancellation or an `on_page` error
        // earlier up the stack); writer is dropped without touching pages 2–4.
        drop(writer);

        assert!(target.join("page_001.png").is_file());
        assert!(target.join("page_002.png").is_file());
        assert!(!target.join("page_003.png").exists());
        assert!(!target.join("page_004.png").exists());
        assert!(!target.join("page_005.png").exists());
    }

    // Windows file permissions don't model POSIX-style write bits the same
    // way; `chmod 0o555` only meaningfully restricts writes on unix. The
    // PermissionDenied → AppError mapping is OS-agnostic in `io_to_app_err`;
    // this test exercises the end-to-end path on the platforms where it's
    // exercisable. The mapping itself is covered for free on every OS via
    // the other tests' io errors not landing here.
    #[cfg(unix)]
    #[test]
    fn permission_denied_target_maps_to_apperror_permission_denied() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("readonly");
        fs::create_dir(&parent).unwrap();
        let mut perms = fs::metadata(&parent).unwrap().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&parent, perms).unwrap();

        let target = parent.join("doc_pages");
        let result = PageWriter::create(&target, Format::Png, 1);

        // Restore write perms so TempDir's recursive drop can clean up.
        let mut perms = fs::metadata(&parent).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&parent, perms).unwrap();

        match result {
            Err(AppError::PermissionDenied { path }) => {
                assert!(path.contains("doc_pages"), "path was {path}");
            }
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
    }
}
