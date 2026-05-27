// Millisecond ↔ `MM:SS.mmm` conversion shared by the media trimmers
// (Audio Trimmer, Video Trimmer). Pure functions, no React — the
// `TimeInput` component in `src/components/TimeInput.tsx` builds on these.

/// Format a duration in milliseconds as `MM:SS.mmm`. Negative inputs
/// clamp to zero.
export function formatMs(ms: number): string {
  const total = Math.max(0, Math.floor(ms));
  const mins = Math.floor(total / 60_000);
  const secs = Math.floor((total % 60_000) / 1000);
  const millis = total % 1000;
  return `${String(mins).padStart(2, "0")}:${String(secs).padStart(2, "0")}.${String(millis).padStart(3, "0")}`;
}

/// Parse `MM:SS` or `MM:SS.mmm` (millis 1–3 digits) into a millisecond
/// count. Returns `null` for malformed input; callers fall back to the
/// previous valid value.
export function parseMs(s: string): number | null {
  const trimmed = s.trim();
  const match = /^(\d+):(\d+)(?:\.(\d{1,3}))?$/.exec(trimmed);
  if (!match) return null;
  const mins = Number(match[1]);
  const secs = Number(match[2]);
  if (secs >= 60) return null;
  const millis = match[3] !== undefined ? Number(match[3].padEnd(3, "0")) : 0;
  return mins * 60_000 + secs * 1000 + millis;
}
