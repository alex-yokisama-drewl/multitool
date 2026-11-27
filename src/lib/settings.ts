// IPC wrapper for the durable settings document.
//
// Boundary file: the only place `@tauri-apps/api` is touched for settings,
// so components and the prefs store stay testable and Playwright keeps one
// mock seam (ARCHITECTURE §3.2). Consumers go through `./prefs`, not this
// file — this is the transport, that is the API.

import { invoke } from "@tauri-apps/api/core";

/**
 * Mirrors `multitool_core::settings::SettingsDoc`. `entries` is an opaque
 * string-keyed blob: Rust validates the envelope and nothing inside it, so
 * every value here is `unknown` until a sanitizer in `./prefs` vouches for
 * it. Field names match the Rust serde output so this stays a thin shape
 * adapter with no renames to drift on.
 */
export interface SettingsDoc {
  version: number;
  entries: Record<string, unknown>;
}

/**
 * Read the settings document.
 *
 * Infallible on the Rust side — a missing, corrupt or foreign-version file
 * resolves to defaults there. A rejection here therefore means the IPC layer
 * itself is unavailable (a plain browser, a test harness), not that the
 * user's settings are bad.
 */
export async function loadSettings(): Promise<SettingsDoc> {
  return await invoke<SettingsDoc>("load_settings");
}

/** Write the whole document. Rejects with an `AppErrorEnvelope`. */
export async function saveSettings(doc: SettingsDoc): Promise<void> {
  await invoke("save_settings", { doc });
}
