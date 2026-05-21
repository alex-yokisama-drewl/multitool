//! Spike test: load the bundled pdfium binary and read a fixture.
//!
//! Verifies the C1 spike — `build.rs` downloaded the right native library for
//! this target, `multitool_core::pdfium::bindings` resolves it, and pdfium
//! can parse a real (if tiny) PDF. Must pass on linux / macos / windows in CI.

use multitool_core::pdfium;
use pdfium_render::prelude::Pdfium;

#[test]
fn binds_native_library_and_reads_page_count() {
    let bindings = pdfium::bindings().expect("bind to pdfium library staged by build.rs");
    let pdfium = Pdfium::new(bindings);

    let pdf = std::fs::read("tests/fixtures/three-page.pdf").expect("read fixture");
    let document = pdfium
        .load_pdf_from_byte_slice(&pdf, None)
        .expect("parse three-page fixture");

    assert_eq!(document.pages().len(), 3);
}
