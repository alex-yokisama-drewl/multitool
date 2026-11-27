// E2E-mode replacement for `src/lib/settings.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true.
// Backed by sessionStorage rather than a module-level variable so a spec can
// assert that a value survives a page reload — which is the browser-side
// stand-in for quitting and relaunching the app, and the only part of the
// settings feature an e2e lane can actually observe.

import type { SettingsDoc } from "@/lib/settings";

const STORAGE_KEY = "multitool-e2e-settings";

export type { SettingsDoc };

export function loadSettings(): Promise<SettingsDoc> {
  const empty: SettingsDoc = { version: 1, entries: {} };
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (raw === null) return Promise.resolve(empty);
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) {
      return Promise.resolve(empty);
    }
    return Promise.resolve(parsed as SettingsDoc);
  } catch {
    return Promise.resolve(empty);
  }
}

export function saveSettings(doc: SettingsDoc): Promise<void> {
  try {
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify(doc));
  } catch {
    // Mirrors the real wrapper's contract: persistence failure is the
    // caller's to shrug off, not to crash on.
  }
  return Promise.resolve();
}
