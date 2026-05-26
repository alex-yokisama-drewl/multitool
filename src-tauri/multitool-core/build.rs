//! Downloads platform-matched native binaries into `OUT_DIR` for the crate.
//!
//! Two payloads:
//! - **pdfium** (library, dynamic-loaded) — fetched from
//!   <https://github.com/bblanchon/pdfium-binaries> at the tag pinned in
//!   [`PDFIUM_TAG`], cached in `OUT_DIR`, and its full path is exported as the
//!   `PDFIUM_LIB_PATH` env var consumed by `src/pdfium.rs` via `env!`.
//!   Decision recorded in `DECISIONS.md` → "pdfium: dynamic-load via build.rs
//!   download".
//! - **ffmpeg** (executable, sidecar) — fetched from
//!   <https://github.com/eugeneware/ffmpeg-static> at the tag pinned in
//!   [`FFMPEG_TAG`], cached in `OUT_DIR`, and its full path is exported as the
//!   `FFMPEG_BIN_PATH` env var consumed by `src/ffmpeg.rs` via `env!`. eugeneware
//!   ships bare binaries (no archive), so the build step is download + chmod.
//!
//! Either path may be supplied directly via env var (`PDFIUM_LIB_PATH` /
//! `FFMPEG_BIN_PATH`) to skip the download — useful for offline builds or when
//! a packaging step has already laid down the binary. The two are independent:
//! overriding one does not affect the other.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

// Matches pdfium-render 0.9.1's default `pdfium_7763` feature. Bump these
// together — the bindings and the binary must agree on the ABI.
//
// `src-tauri/build.rs` mirrors this download to stage the binary as a Tauri
// resource for bundled installs (DECISIONS.md → "pdfium: bundle native
// binary as a Tauri resource"). Bump `PDFIUM_TAG` in both files together.
const PDFIUM_TAG: &str = "chromium/7763";

// eugeneware/ffmpeg-static release tag. `b<X>` prefix means "binaries for
// ffmpeg X". b6.1.1 ships ffmpeg 6.1.1 — sufficient for the H.264 / VP9 /
// Opus / AAC codecs the Video Format Converter targets.
//
// `src-tauri/build.rs` mirrors this download to stage the binary as a Tauri
// resource for bundled installs. Bump `FFMPEG_TAG` in both files together.
const FFMPEG_TAG: &str = "b6.1.1";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PDFIUM_LIB_PATH");
    println!("cargo:rerun-if-env-changed=FFMPEG_BIN_PATH");

    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS set by cargo");
    let target_arch =
        env::var("CARGO_CFG_TARGET_ARCH").expect("CARGO_CFG_TARGET_ARCH set by cargo");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR set by cargo"));

    prepare_pdfium(&target_os, &target_arch, &out_dir);
    prepare_ffmpeg(&target_os, &target_arch, &out_dir);
}

fn prepare_pdfium(target_os: &str, target_arch: &str, out_dir: &Path) {
    if let Ok(prebuilt) = env::var("PDFIUM_LIB_PATH") {
        // Trust the caller; they took responsibility for placing the binary.
        println!("cargo:rustc-env=PDFIUM_LIB_PATH={prebuilt}");
        return;
    }

    let (asset, lib_rel_path) = match (target_os, target_arch) {
        ("linux", "x86_64") => ("pdfium-linux-x64.tgz", "lib/libpdfium.so"),
        ("linux", "aarch64") => ("pdfium-linux-arm64.tgz", "lib/libpdfium.so"),
        ("macos", "x86_64") => ("pdfium-mac-x64.tgz", "lib/libpdfium.dylib"),
        ("macos", "aarch64") => ("pdfium-mac-arm64.tgz", "lib/libpdfium.dylib"),
        ("windows", "x86_64") => ("pdfium-win-x64.tgz", "bin/pdfium.dll"),
        (os, arch) => panic!(
            "multitool-core: no bblanchon prebuilt pdfium for target {os}/{arch}. \
             Set PDFIUM_LIB_PATH to point at a locally-built binary to override."
        ),
    };

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

    println!("cargo:rustc-env=PDFIUM_LIB_PATH={}", lib_path.display());
}

fn prepare_ffmpeg(target_os: &str, target_arch: &str, out_dir: &Path) {
    if let Ok(prebuilt) = env::var("FFMPEG_BIN_PATH") {
        println!("cargo:rustc-env=FFMPEG_BIN_PATH={prebuilt}");
        return;
    }

    // eugeneware/ffmpeg-static asset names + the on-disk filename we want.
    // Windows refuses to spawn a PE binary without a `.exe` extension, so we
    // rename at extract time. Linux/macOS keep the bare `ffmpeg` name.
    let (asset, dest_name) = match (target_os, target_arch) {
        ("linux", "x86_64") => ("ffmpeg-linux-x64", "ffmpeg"),
        ("linux", "aarch64") => ("ffmpeg-linux-arm64", "ffmpeg"),
        ("macos", "x86_64") => ("ffmpeg-darwin-x64", "ffmpeg"),
        ("macos", "aarch64") => ("ffmpeg-darwin-arm64", "ffmpeg"),
        ("windows", "x86_64") => ("ffmpeg-win32-x64", "ffmpeg.exe"),
        (os, arch) => panic!(
            "multitool-core: no eugeneware prebuilt ffmpeg for target {os}/{arch}. \
             Set FFMPEG_BIN_PATH to point at a local ffmpeg binary to override."
        ),
    };

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

    println!("cargo:rustc-env=FFMPEG_BIN_PATH={}", bin_path.display());
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
    // Windows derives executability from the file extension, not a mode bit.
    // The `dest_name` mapping above already gives PE files the `.exe`
    // extension Windows requires.
}
