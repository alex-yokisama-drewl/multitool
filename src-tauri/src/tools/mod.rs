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

pub mod audio_extractor;
pub mod audio_format_converter;
pub mod audio_trimmer;
pub mod image_compress;
pub mod image_crop;
pub mod image_format_converter;
pub mod image_resize;
pub mod images_to_pdf;
pub mod pdf_to_images;
pub mod video_compress;
pub mod video_format_converter;
pub mod video_trimmer;

pub fn register_commands<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
        crate::ipc::cancel_job,
        crate::asset_scope::allow_media_preview,
        crate::preview_proxy::prepare_preview_proxy,
        crate::preview_proxy::cleanup_preview_proxy,
        crate::preview_proxy::cleanup_stale_proxies,
        crate::settings::load_settings,
        crate::settings::save_settings,
        crate::system::supported_raster_formats,
        crate::system::file_size,
        audio_extractor::extract_audio,
        audio_format_converter::convert_audio_format,
        audio_trimmer::trim_audio,
        image_compress::compress_images,
        image_crop::crop_image,
        image_format_converter::convert_image_format,
        image_resize::resize_image,
        images_to_pdf::convert_images_to_pdf,
        pdf_to_images::convert_pdf_to_images,
        video_compress::compress_video,
        video_format_converter::convert_video_format,
        video_trimmer::trim_video,
        video_trimmer::probe_video_duration,
    ])
}
