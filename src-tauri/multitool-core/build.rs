//! Downloads the pdfium native binary matching the host target into `OUT_DIR`.
//!
//! Decision recorded in `DECISIONS.md` → "pdfium: dynamic-load via build.rs download".
//! The binary is fetched from <https://github.com/bblanchon/pdfium-binaries>
//! at the tag pinned in `PDFIUM_TAG`, cached in `OUT_DIR`, and its full path is
//! exported as the `PDFIUM_LIB_PATH` env var consumed by `src/pdfium.rs` via
//! `env!`. Subsequent rebuilds skip the download as long as the extracted
//! library file is still on disk.
//!
//! `PDFIUM_LIB_PATH` may be set in the environment before running cargo, in
//! which case the download is skipped entirely — useful for offline builds or
//! when a packaging step has already laid down the binary.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

// Matches pdfium-render 0.9.1's default `pdfium_7763` feature. Bump these
// together — the bindings and the binary must agree on the ABI.
const PDFIUM_TAG: &str = "chromium/7763";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PDFIUM_LIB_PATH");

    if let Ok(prebuilt) = env::var("PDFIUM_LIB_PATH") {
        // Trust the caller; they took responsibility for placing the binary.
        println!("cargo:rustc-env=PDFIUM_LIB_PATH={prebuilt}");
        return;
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR set by cargo"));
    let extract_dir = out_dir.join("pdfium");

    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS set by cargo");
    let target_arch =
        env::var("CARGO_CFG_TARGET_ARCH").expect("CARGO_CFG_TARGET_ARCH set by cargo");

    let (asset, lib_rel_path) = match (target_os.as_str(), target_arch.as_str()) {
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
