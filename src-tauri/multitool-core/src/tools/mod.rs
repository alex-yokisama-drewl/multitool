//! Pure-logic implementations of each tool.
//!
//! One module per tool — these mirror `src-tauri/src/tools/<tool>/` on the
//! shell side. Heavy work (PDF rendering, image encoding, …) lives here so it
//! is testable without `tauri::test`. The shell modules are thin Tauri-command
//! adapters that delegate into these.
//!
//! Adding a tool: `pub mod <tool_name>;` and you're done — there is no central
//! registry on this side. The registry pattern is enforced on the shell side
//! (`src-tauri/src/tools/mod.rs::register_commands`).

pub mod audio_extractor;
pub mod audio_format_converter;
pub mod audio_trimmer;
pub mod image_crop;
pub mod image_format_converter;
pub mod images_to_pdf;
pub mod pdf_to_images;
pub mod video_format_converter;
