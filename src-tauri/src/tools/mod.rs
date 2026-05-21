//! Tool registry.
//!
//! To add a tool:
//!   1. Add `pub mod <tool_name>;` below.
//!   2. Append the tool's `#[tauri::command]` functions to the
//!      `generate_handler!` invocation in [`register_commands`].
//!
//! No other file should change to add a tool — that is the contract that
//! keeps the modular architecture from SPEC §5.1 honest.

pub fn register_commands<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.invoke_handler(tauri::generate_handler![crate::ipc::cancel_job,])
}
