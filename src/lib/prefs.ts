// Remembered user preferences: the frontend half of the settings feature.
//
// The Rust side stores an opaque string-keyed blob (see
// `multitool-core/src/settings.rs`). Every schema lives here, which is what
// keeps the registry contract from ARCHITECTURE §3.1 intact — a new tool
// declares its own prefs in its own folder and no shared Rust code changes.
//
// Three rules the rest of the app relies on:
//
//  1. **Nothing is trusted on read.** A stored value is `unknown`. It was
//     written by an older build, or hand-edited, or belongs to an option
//     that no longer exists. Every field passes through a sanitizer that
//     falls back to the default rather than forwarding junk to Rust.
//  2. **Nothing is written before hydration.** If the document never
//     loaded, writing would clobber real settings with defaults. Until
//     hydration succeeds the store is a plain in-memory session store.
//  3. **Persistence failure is never fatal.** A read-only config dir costs
//     the user their memory, not their session.

import { useCallback, useMemo, useSyncExternalStore } from "react";
import { create } from "zustand";
// Imported by its aliased path, not relatively: `vite.config.ts` swaps
// `@/lib/settings` for the e2e mock by exact specifier, and `./settings`
// would slip past that alias and reach the real IPC wrapper in the e2e
// lane. The same applies to any future module here that the e2e lane
// needs to stub.
import { loadSettings, saveSettings, type SettingsDoc } from "@/lib/settings";

/**
 * How long to sit on a change before writing. Long enough that dragging a
 * quality slider is one write rather than fifty; short enough that the write
 * lands well before a user can change a value and quit the app.
 */
const WRITE_DEBOUNCE_MS = 400;

interface PrefsState {
  entries: Record<string, unknown>;
  /**
   * True once the document has been read from disk. Gates persistence —
   * see rule 2 above. Stays false when IPC is unavailable, which makes the
   * store an in-memory session store for Vitest and plain-browser runs.
   */
  hydrated: boolean;
  hydrate: (entries: Record<string, unknown>) => void;
  put: (key: string, value: unknown) => void;
}

export const usePrefsStore = create<PrefsState>((set) => ({
  entries: {},
  hydrated: false,
  hydrate: (entries) => {
    set({ entries, hydrated: true });
  },
  put: (key, value) => {
    set((state) => ({ entries: { ...state.entries, [key]: value } }));
  },
}));

// --- persistence -----------------------------------------------------------

let writeTimer: ReturnType<typeof setTimeout> | null = null;
let pendingWrite: Promise<void> = Promise.resolve();

function currentDoc(): SettingsDoc {
  return { version: 1, entries: usePrefsStore.getState().entries };
}

function writeNow(): Promise<void> {
  if (!usePrefsStore.getState().hydrated) return Promise.resolve();
  pendingWrite = saveSettings(currentDoc()).catch((err: unknown) => {
    // Non-blocking by design: the session keeps its preferences in memory,
    // they just won't outlive it. Nothing in the UI branches on this.
    console.warn("multitool: could not persist settings", err);
  });
  return pendingWrite;
}

function scheduleWrite(): void {
  if (writeTimer !== null) clearTimeout(writeTimer);
  writeTimer = setTimeout(() => {
    writeTimer = null;
    void writeNow();
  }, WRITE_DEBOUNCE_MS);
}

/**
 * Read the settings document into the store. Call once, before first paint,
 * so tools render their remembered values instead of flashing defaults.
 *
 * Resolves either way: a rejection means IPC is unavailable, which leaves
 * the store unhydrated and therefore read-only on disk.
 */
export async function hydratePrefs(): Promise<void> {
  try {
    const doc = await loadSettings();
    usePrefsStore.getState().hydrate(doc.entries);
  } catch (err: unknown) {
    console.warn("multitool: could not load settings; using defaults", err);
  }
}

/**
 * Flush any debounced write immediately and wait for it. Exists for tests
 * and for callers that need the document on disk before continuing.
 */
export async function flushPrefs(): Promise<void> {
  if (writeTimer !== null) {
    clearTimeout(writeTimer);
    writeTimer = null;
    await writeNow();
  }
  await pendingWrite;
}

/** Test seam: drop in-memory state and any scheduled write. */
export function resetPrefsForTest(): void {
  if (writeTimer !== null) clearTimeout(writeTimer);
  writeTimer = null;
  pendingWrite = Promise.resolve();
  usePrefsStore.setState({ entries: {}, hydrated: false });
}

// --- sanitizers ------------------------------------------------------------

/**
 * Vets one stored value. Receives whatever was on disk (`unknown`) plus the
 * default to fall back to, and must return a value of the right type —
 * never throw.
 */
export type Sanitizer<T> = (raw: unknown, fallback: T) => T;

export type Sanitizers<T> = { [K in keyof T]: Sanitizer<T[K]> };

/**
 * Accept only one of a known set. The workhorse for every enum-shaped
 * option (target format, fit mode, …): a value whose variant was removed in
 * a later build falls back instead of reaching Rust.
 */
export function oneOf<T extends string>(allowed: readonly T[]): Sanitizer<T> {
  return (raw, fallback) =>
    typeof raw === "string" && (allowed as readonly string[]).includes(raw)
      ? (raw as T)
      : fallback;
}

/**
 * Accept a finite number, clamped into `[min, max]`. `integer` rounds, for
 * options Rust expects as whole numbers (DPI, bitrate, pixel sizes).
 */
export function numberIn(
  min: number,
  max: number,
  { integer = false }: { integer?: boolean } = {},
): Sanitizer<number> {
  return (raw, fallback) => {
    if (typeof raw !== "number" || !Number.isFinite(raw)) return fallback;
    const clamped = Math.min(max, Math.max(min, raw));
    return integer ? Math.round(clamped) : clamped;
  };
}

/**
 * Accept any finite number, optionally rounded — no range clamp.
 *
 * For options bound to a free-typed `<input type="number">`. Clamping those
 * on read would fight the user mid-keystroke: typing `7` on the way to `700`
 * would snap the field to the minimum, and the next keystroke would compound
 * it. The real range clamp for these already lives in Rust (DPI, resize
 * dimensions and the quality dials all clamp silently in `convert`), so this
 * only has to guarantee a number.
 *
 * Use {@link numberIn} instead whenever the control itself constrains the
 * value — a slider can't produce an out-of-range number, so clamping it
 * costs nothing.
 */
export function finiteNumber({
  integer = false,
}: { integer?: boolean } = {}): Sanitizer<number> {
  return (raw, fallback) => {
    if (typeof raw !== "number" || !Number.isFinite(raw)) return fallback;
    return integer ? Math.round(raw) : raw;
  };
}

export function bool(): Sanitizer<boolean> {
  return (raw, fallback) => (typeof raw === "boolean" ? raw : fallback);
}

/** Accept a non-empty string. Used for remembered directory paths. */
export function nonEmptyString(): Sanitizer<string> {
  return (raw, fallback) =>
    typeof raw === "string" && raw.length > 0 ? raw : fallback;
}

// --- pref groups -----------------------------------------------------------

export type PrefValues = Record<string, unknown>;

export interface PrefGroup<T extends PrefValues> {
  /** Storage key inside the settings document. */
  readonly key: string;
  readonly defaults: T;
  /** Sanitized current values. Safe to call outside React. */
  read(): T;
  /** Overwrite one field and schedule a write. Safe to call outside React. */
  write<K extends keyof T>(field: K, value: T[K]): void;
  /** React binding: `[values, setField]`. */
  use(): [T, <K extends keyof T>(field: K, value: T[K]) => void];
}

/**
 * Declare a group of related preferences stored under one key.
 *
 * One key per group rather than per field keeps the document small, makes a
 * write atomic across related options, and means a tool's whole memory can
 * be dropped by deleting a single entry.
 *
 * ```ts
 * const prefs = definePrefs(
 *   "tool:pdf-to-images",
 *   { format: "png", dpi: 150 },
 *   { format: oneOf<Format>(["png", "jpeg"]), dpi: numberIn(72, 600, { integer: true }) },
 * );
 * ```
 *
 * The value shape is inferred from `sanitizers`, not from `defaults`
 * (`NoInfer`): sanitizers carry the literal union types, while an inline
 * default like `"png"` would widen to `string` and lose them.
 */
export function definePrefs<T extends PrefValues>(
  key: string,
  defaults: NoInfer<T>,
  sanitizers: Sanitizers<T>,
): PrefGroup<T> {
  function sanitize(raw: unknown): T {
    const stored =
      typeof raw === "object" && raw !== null && !Array.isArray(raw)
        ? (raw as Record<string, unknown>)
        : {};
    const out = {} as T;
    for (const field of Object.keys(defaults) as (keyof T)[]) {
      const fallback = defaults[field];
      out[field] = sanitizers[field](stored[field as string], fallback);
    }
    return out;
  }

  function rawEntry(): unknown {
    return usePrefsStore.getState().entries[key];
  }

  function read(): T {
    return sanitize(rawEntry());
  }

  function write<K extends keyof T>(field: K, value: T[K]): void {
    usePrefsStore.getState().put(key, { ...read(), [field]: value });
    scheduleWrite();
  }

  function use(): [T, <K extends keyof T>(field: K, value: T[K]) => void] {
    // Subscribe to the raw entry, not the sanitized object: sanitizing
    // allocates a fresh object every call, and useSyncExternalStore
    // compares snapshots by identity — returning a new one each time is an
    // infinite render loop. The raw value is stable between writes.
    const raw = useSyncExternalStore(
      usePrefsStore.subscribe,
      rawEntry,
      rawEntry,
    );
    const values = useMemo(() => sanitize(raw), [raw]);
    const setField = useCallback(<K extends keyof T>(field: K, value: T[K]) => {
      write(field, value);
    }, []);
    return [values, setField];
  }

  return { key, defaults, read, write, use };
}
