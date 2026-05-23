//! Tauri shell build script.
//!
//! Responsibilities:
//! 1. Run `tauri_build::build()` so the Tauri macros pick up the config.
//! 2. Stage the pdfium native binary under `resources/pdfium/` so
//!    `bundle.resources` in `tauri.conf.json` bundles it alongside the
//!    installed app. At runtime, [`crate::run`] resolves the bundled path
//!    and hands it to `multitool_core::pdfium::init`, which fixes the
//!    "pdfium DLL not found" failure that shipped in v0.2.0
//!    (DECISIONS.md → "pdfium: bundle native binary as a Tauri resource").
//!
//! The download and extract logic deliberately mirrors
//! `multitool-core/build.rs`. The two crates have separate `OUT_DIR`s and
//! cargo gives us no clean way to share a downloaded artifact between
//! build scripts — duplicating ~30 lines is cheaper than bolting a
//! shared workspace path on top of cargo's per-crate model. If the
//! pdfium pin ever moves, update both build scripts together.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const PDFIUM_TAG: &str = "chromium/7763";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PDFIUM_LIB_PATH");

    stage_pdfium_resource();

    tauri_build::build();
}

fn stage_pdfium_resource() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS set by cargo");
    let target_arch =
        env::var("CARGO_CFG_TARGET_ARCH").expect("CARGO_CFG_TARGET_ARCH set by cargo");

    let (asset, lib_rel_path, dest_name) = match (target_os.as_str(), target_arch.as_str()) {
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

    let crate_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let staged_dir = crate_dir.join("resources").join("pdfium");
    let staged_path = staged_dir.join(dest_name);

    // Source of the binary: prefer an explicit override (offline/CI cache),
    // otherwise download into this crate's `OUT_DIR` and cache there.
    let source_path = if let Ok(prebuilt) = env::var("PDFIUM_LIB_PATH") {
        PathBuf::from(prebuilt)
    } else {
        let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR set by cargo"));
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

    // Copy unconditionally rather than skip-when-exists — picks up upstream
    // pin bumps and stays correct if the source file was replaced. The file
    // is small (~5 MB) so the copy is cheap.
    fs::copy(&source_path, &staged_path).unwrap_or_else(|err| {
        panic!(
            "copy pdfium binary {} -> {}: {err}",
            source_path.display(),
            staged_path.display()
        )
    });

    println!("cargo:rerun-if-changed={}", source_path.display());
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
