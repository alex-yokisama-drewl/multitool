//! Tool registry.
//!
//! To add a tool:
//!   1. Add `pub mod <tool_name>;` below.
//!   2. Append the tool's `#[tauri::command]` functions to the
//!      `generate_handler!` invocation in [`register_commands`].
//!
//! No other file should change to add a tool — that is the contract that
//! keeps the modular architecture from ARCHITECTURE §3.1 honest. The `generate_handler!`
//! macro is checked at compile time, so a misspelled command path or a
//! `#[tauri::command]` signature that drifted will fail `cargo build` /
//! `cargo clippy` before any UI loads.

pub mod pdf_to_images;

pub fn register_commands<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
        crate::ipc::cancel_job,
        pdf_to_images::convert_pdf_to_images,
    ])
}
