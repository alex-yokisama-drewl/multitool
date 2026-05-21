#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

mod error;
pub mod fs;
pub mod ipc;
pub mod tools;

pub use error::{AppError, AppResult};

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // `try_init` is a no-op if a subscriber is already installed (tests, etc.).
    let _ = fmt().with_env_filter(filter).try_init();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    let builder = tauri::Builder::default().manage(ipc::JobRegistry::default());
    let builder = tools::register_commands(builder);

    if let Err(err) = builder.run(tauri::generate_context!()) {
        eprintln!("fatal: failed to run tauri application: {err}");
        std::process::exit(1);
    }
}
