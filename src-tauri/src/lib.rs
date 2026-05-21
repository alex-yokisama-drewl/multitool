#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(err) = tauri::Builder::default().run(tauri::generate_context!()) {
        eprintln!("fatal: failed to run tauri application: {err}");
        std::process::exit(1);
    }
}
