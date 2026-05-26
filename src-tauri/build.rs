//! Tauri shell build script.
//!
//! Responsibilities:
//! 1. Run `tauri_build::build()` so the Tauri macros pick up the config.
//! 2. Stage the pdfium native library under `resources/pdfium/` so
//!    `bundle.resources` in `tauri.conf.json` bundles it alongside the
//!    installed app. At runtime, [`crate::run`] resolves the bundled path
//!    and hands it to `multitool_core::pdfium::init`, which fixes the
//!    "pdfium DLL not found" failure that shipped in v0.2.0
//!    (DECISIONS.md → "pdfium: bundle native binary as a Tauri resource").
//! 3. Stage the ffmpeg sidecar binary under `resources/ffmpeg/` for the same
//!    reason — bundled installs spawn the staged binary as a subprocess for
//!    the Video Format Converter tool.
//!
//! The download logic deliberately mirrors `multitool-core/build.rs`. The
//! two crates have separate `OUT_DIR`s and cargo gives us no clean way to
//! share a downloaded artifact between build scripts — duplicating a few
//! dozen lines is cheaper than bolting a shared workspace path on top of
//! cargo's per-crate model. If either pin moves, update both build scripts
//! together.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const PDFIUM_TAG: &str = "chromium/7763";
const FFMPEG_TAG: &str = "b6.1.1";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PDFIUM_LIB_PATH");
    println!("cargo:rerun-if-env-changed=FFMPEG_BIN_PATH");

    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS set by cargo");
    let target_arch =
        env::var("CARGO_CFG_TARGET_ARCH").expect("CARGO_CFG_TARGET_ARCH set by cargo");
    let crate_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR set by cargo"));

    stage_pdfium_resource(&target_os, &target_arch, &crate_dir, &out_dir);
    stage_ffmpeg_resource(&target_os, &target_arch, &crate_dir, &out_dir);

    tauri_build::build();
}

fn stage_pdfium_resource(target_os: &str, target_arch: &str, crate_dir: &Path, out_dir: &Path) {
    let (asset, lib_rel_path, dest_name) = match (target_os, target_arch) {
        ("linux", "x86_64") => ("pdfium-linux-x64.tgz", "lib/libpdfium.so", "libpdfium.so"),
        ("linux", "aarch64") => ("pdfium-linux-arm64.tgz", "lib/libpdfium.so", "libpdfium.so"),
        ("macos", "x86_64") => (
            "pdfium-mac-x64.tgz",
            "lib/libpdfium.dylib",
            "libpdfium.dylib",
        ),
        ("macos", "aarch64") => (
            "pdfium-mac-arm64.tgz",
            "lib/libpdfium.dylib",
            "libpdfium.dylib",
        ),
        ("windows", "x86_64") => ("pdfium-win-x64.tgz", "bin/pdfium.dll", "pdfium.dll"),
        (os, arch) => panic!(
            "src-tauri build.rs: no bblanchon prebuilt pdfium for target {os}/{arch}. \
             Set PDFIUM_LIB_PATH to point at a locally-built binary to override."
        ),
    };

    let staged_dir = crate_dir.join("resources").join("pdfium");
    let staged_path = staged_dir.join(dest_name);

    // Source of the binary: prefer an explicit override (offline/CI cache),
    // otherwise download into this crate's `OUT_DIR` and cache there.
    let source_path = if let Ok(prebuilt) = env::var("PDFIUM_LIB_PATH") {
        PathBuf::from(prebuilt)
    } else {
        let extract_dir = out_dir.join("pdfium");
        let lib_path = extract_dir.join(lib_rel_path);
        if !lib_path.exists() {
            let url = format!(
                "https://github.com/bblanchon/pdfium-binaries/releases/download/{tag}/{asset}",
                tag = PDFIUM_TAG,
            );
            fs::create_dir_all(&extract_dir).expect("create pdfium extract dir");
            download_and_extract_tgz(&url, &extract_dir);
            assert!(
                lib_path.exists(),
                "pdfium binary not found at {} after extracting {asset}",
                lib_path.display(),
            );
        }
        lib_path
    };

    fs::create_dir_all(&staged_dir).expect("create resources/pdfium dir");

    // Idempotent copy — overwriting on every build touches the staged file's
    // mtime, and the staged file lives under `src-tauri/`, which Tauri's
    // dev-mode watcher monitors. An unconditional copy made `pnpm tauri dev`
    // wedge in a rebuild loop. Skip when the dest is plausibly already
    // current (same size, dest newer than src). Pin bumps either change the
    // file size (most common) or produce a strictly newer source mtime, so
    // both paths still trigger a re-copy.
    if needs_copy(&source_path, &staged_path) {
        fs::copy(&source_path, &staged_path).unwrap_or_else(|err| {
            panic!(
                "copy pdfium binary {} -> {}: {err}",
                source_path.display(),
                staged_path.display()
            )
        });
    }

    println!("cargo:rerun-if-changed={}", source_path.display());
}

fn stage_ffmpeg_resource(target_os: &str, target_arch: &str, crate_dir: &Path, out_dir: &Path) {
    let (asset, dest_name) = match (target_os, target_arch) {
        ("linux", "x86_64") => ("ffmpeg-linux-x64", "ffmpeg"),
        ("linux", "aarch64") => ("ffmpeg-linux-arm64", "ffmpeg"),
        ("macos", "x86_64") => ("ffmpeg-darwin-x64", "ffmpeg"),
        ("macos", "aarch64") => ("ffmpeg-darwin-arm64", "ffmpeg"),
        ("windows", "x86_64") => ("ffmpeg-win32-x64", "ffmpeg.exe"),
        (os, arch) => panic!(
            "src-tauri build.rs: no eugeneware prebuilt ffmpeg for target {os}/{arch}. \
             Set FFMPEG_BIN_PATH to point at a local ffmpeg binary to override."
        ),
    };

    let staged_dir = crate_dir.join("resources").join("ffmpeg");
    let staged_path = staged_dir.join(dest_name);

    let source_path = if let Ok(prebuilt) = env::var("FFMPEG_BIN_PATH") {
        PathBuf::from(prebuilt)
    } else {
        let ffmpeg_dir = out_dir.join("ffmpeg");
        let bin_path = ffmpeg_dir.join(dest_name);
        if !bin_path.exists() {
            let url = format!(
                "https://github.com/eugeneware/ffmpeg-static/releases/download/{tag}/{asset}",
                tag = FFMPEG_TAG,
            );
            fs::create_dir_all(&ffmpeg_dir).expect("create ffmpeg cache dir");
            download_binary(&url, &bin_path);
            make_executable(&bin_path);
            assert!(
                bin_path.exists(),
                "ffmpeg binary not found at {} after download",
                bin_path.display(),
            );
        }
        bin_path
    };

    fs::create_dir_all(&staged_dir).expect("create resources/ffmpeg dir");

    if needs_copy(&source_path, &staged_path) {
        fs::copy(&source_path, &staged_path).unwrap_or_else(|err| {
            panic!(
                "copy ffmpeg binary {} -> {}: {err}",
                source_path.display(),
                staged_path.display()
            )
        });
        // `fs::copy` preserves Unix mode bits, so the staged file inherits
        // the 0755 we set during download. Re-apply defensively in case the
        // override path was supplied with a non-executable mode.
        make_executable(&staged_path);
    }

    println!("cargo:rerun-if-changed={}", source_path.display());
}

fn needs_copy(src: &Path, dest: &Path) -> bool {
    let Ok(src_meta) = fs::metadata(src) else {
        // No source — nothing we can usefully do here; let the caller
        // proceed (the earlier extract assertion would have caught a
        // missing source).
        return false;
    };
    let Ok(dest_meta) = fs::metadata(dest) else {
        return true;
    };
    if src_meta.len() != dest_meta.len() {
        return true;
    }
    // Modified-time check: re-copy if the source is strictly newer than
    // the staged file. mtime errors fall back to "copy anyway" — unusual
    // filesystem but safer than silently going stale.
    let (Ok(src_mt), Ok(dest_mt)) = (src_meta.modified(), dest_meta.modified()) else {
        return true;
    };
    src_mt > dest_mt
}

fn download_and_extract_tgz(url: &str, out: &Path) {
    let response = ureq::get(url)
        .call()
        .unwrap_or_else(|err| panic!("failed to download {url}: {err}"));
    let reader = response.into_reader();
    let gz = flate2::read::GzDecoder::new(reader);
    let mut archive = tar::Archive::new(gz);
    archive.unpack(out).unwrap_or_else(|err| {
        panic!(
            "failed to extract pdfium tarball into {}: {err}",
            out.display()
        )
    });
}

fn download_binary(url: &str, dest: &Path) {
    let response = ureq::get(url)
        .call()
        .unwrap_or_else(|err| panic!("failed to download {url}: {err}"));
    let mut reader = response.into_reader();
    let mut file =
        fs::File::create(dest).unwrap_or_else(|err| panic!("create {}: {err}", dest.display()));
    std::io::copy(&mut reader, &mut file)
        .unwrap_or_else(|err| panic!("write {}: {err}", dest.display()));
    file.flush()
        .unwrap_or_else(|err| panic!("flush {}: {err}", dest.display()));
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = fs::metadata(path)
        .unwrap_or_else(|err| panic!("stat {}: {err}", path.display()))
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)
        .unwrap_or_else(|err| panic!("chmod 0755 {}: {err}", path.display()));
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {
    // Windows derives executability from the file extension; we name the
    // staged file `ffmpeg.exe` above to satisfy that.
}
