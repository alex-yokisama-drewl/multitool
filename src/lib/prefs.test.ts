import { renderHook, act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  loadSettings: vi.fn(),
  saveSettings: vi.fn(),
}));

vi.mock("./settings", () => ({
  loadSettings: mocks.loadSettings,
  saveSettings: mocks.saveSettings,
}));

import {
  bool,
  definePrefs,
  flushPrefs,
  hydratePrefs,
  nonEmptyString,
  numberIn,
  oneOf,
  resetPrefsForTest,
  usePrefsStore,
} from "./prefs";

type Format = "png" | "jpeg";

function makeGroup(key = "tool:example") {
  return definePrefs(
    key,
    { format: "png", dpi: 150, locked: false },
    {
      format: oneOf<Format>(["png", "jpeg"]),
      dpi: numberIn(72, 600, { integer: true }),
      locked: bool(),
    },
  );
}

/** Put the store in the hydrated state a running app would be in. */
async function hydrateWith(entries: Record<string, unknown>) {
  mocks.loadSettings.mockResolvedValue({ version: 1, entries });
  await hydratePrefs();
}

beforeEach(() => {
  vi.useFakeTimers();
  resetPrefsForTest();
  mocks.loadSettings.mockReset();
  mocks.saveSettings.mockReset();
  mocks.saveSettings.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("sanitizers", () => {
  it("oneOf keeps a known variant and rejects anything else", () => {
    const s = oneOf<Format>(["png", "jpeg"]);
    expect(s("jpeg", "png")).toBe("jpeg");
    expect(s("heic", "png")).toBe("png");
    expect(s(42, "png")).toBe("png");
    expect(s(undefined, "png")).toBe("png");
    expect(s(null, "png")).toBe("png");
  });

  it("numberIn clamps into range and rounds when integer", () => {
    const s = numberIn(72, 600, { integer: true });
    expect(s(300, 150)).toBe(300);
    expect(s(5000, 150)).toBe(600);
    expect(s(1, 150)).toBe(72);
    expect(s(149.6, 150)).toBe(150);
    expect(s("300", 150)).toBe(150);
    expect(s(Number.NaN, 150)).toBe(150);
    expect(s(Number.POSITIVE_INFINITY, 150)).toBe(150);
  });

  it("numberIn keeps fractions when not integer", () => {
    const s = numberIn(0, 1);
    expect(s(0.35, 0.8)).toBe(0.35);
    expect(s(2, 0.8)).toBe(1);
  });

  it("bool and nonEmptyString reject off-type values", () => {
    expect(bool()(true, false)).toBe(true);
    expect(bool()("true", false)).toBe(false);
    expect(nonEmptyString()("/home/x", "/fallback")).toBe("/home/x");
    expect(nonEmptyString()("", "/fallback")).toBe("/fallback");
    expect(nonEmptyString()(7, "/fallback")).toBe("/fallback");
  });
});

describe("definePrefs read", () => {
  it("returns defaults when nothing is stored", () => {
    expect(makeGroup().read()).toEqual({
      format: "png",
      dpi: 150,
      locked: false,
    });
  });

  it("returns stored values once hydrated", async () => {
    await hydrateWith({ "tool:example": { format: "jpeg", dpi: 300 } });
    expect(makeGroup().read()).toEqual({
      format: "jpeg",
      dpi: 300,
      locked: false,
    });
  });

  it("falls back per field, keeping the good values beside the bad", async () => {
    await hydrateWith({
      "tool:example": { format: "heic", dpi: 9999, locked: "yes" },
    });
    expect(makeGroup().read()).toEqual({
      format: "png", // unknown variant → default
      dpi: 600, // out of range → clamped
      locked: false, // wrong type → default
    });
  });

  it("ignores a stored entry that is not an object", async () => {
    await hydrateWith({ "tool:example": "corrupt" });
    expect(makeGroup().read()).toEqual(makeGroup().defaults);
  });

  it("ignores unknown fields left by an older build", async () => {
    await hydrateWith({
      "tool:example": { format: "jpeg", removedOption: "x" },
    });
    expect(makeGroup().read()).not.toHaveProperty("removedOption");
  });

  it("keeps groups isolated by key", async () => {
    await hydrateWith({ "tool:a": { dpi: 300 }, "tool:b": { dpi: 400 } });
    expect(makeGroup("tool:a").read().dpi).toBe(300);
    expect(makeGroup("tool:b").read().dpi).toBe(400);
  });
});

describe("definePrefs write", () => {
  it("does not persist before hydration", async () => {
    makeGroup().write("dpi", 300);
    await vi.advanceTimersByTimeAsync(1000);
    expect(mocks.saveSettings).not.toHaveBeenCalled();
  });

  it("still serves the new value in-session when unhydrated", () => {
    const group = makeGroup();
    group.write("dpi", 300);
    expect(group.read().dpi).toBe(300);
  });

  it("persists after the debounce once hydrated", async () => {
    await hydrateWith({});
    makeGroup().write("dpi", 300);

    expect(mocks.saveSettings).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(400);

    expect(mocks.saveSettings).toHaveBeenCalledTimes(1);
    expect(mocks.saveSettings.mock.calls[0]?.[0]).toEqual({
      version: 1,
      entries: { "tool:example": { format: "png", dpi: 300, locked: false } },
    });
  });

  it("coalesces a burst of writes into one save", async () => {
    await hydrateWith({});
    const group = makeGroup();

    for (const dpi of [100, 200, 300, 400]) group.write("dpi", dpi);
    await vi.advanceTimersByTimeAsync(400);

    expect(mocks.saveSettings).toHaveBeenCalledTimes(1);
    expect(group.read().dpi).toBe(400);
  });

  it("preserves other groups' entries when writing one", async () => {
    await hydrateWith({ "tool:other": { keep: true } });
    makeGroup().write("dpi", 300);
    await vi.advanceTimersByTimeAsync(400);

    const doc = mocks.saveSettings.mock.calls[0]?.[0] as {
      entries: Record<string, unknown>;
    };
    expect(doc.entries["tool:other"]).toEqual({ keep: true });
  });

  it("survives a rejected save without throwing", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    mocks.saveSettings.mockRejectedValue({ kind: "PermissionDenied" });
    await hydrateWith({});

    makeGroup().write("dpi", 300);
    await expect(flushPrefs()).resolves.toBeUndefined();
    expect(makeGroup().read().dpi).toBe(300);
  });

  it("flushPrefs writes immediately instead of waiting out the debounce", async () => {
    await hydrateWith({});
    makeGroup().write("dpi", 300);

    await flushPrefs();

    expect(mocks.saveSettings).toHaveBeenCalledTimes(1);
  });
});

describe("hydratePrefs", () => {
  it("marks the store hydrated on success", async () => {
    await hydrateWith({ k: 1 });
    expect(usePrefsStore.getState().hydrated).toBe(true);
  });

  it("leaves the store unhydrated and silent when IPC is unavailable", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    mocks.loadSettings.mockRejectedValue(new Error("no tauri"));

    await expect(hydratePrefs()).resolves.toBeUndefined();

    expect(usePrefsStore.getState().hydrated).toBe(false);
    makeGroup().write("dpi", 300);
    await vi.advanceTimersByTimeAsync(1000);
    expect(mocks.saveSettings).not.toHaveBeenCalled();
  });
});

describe("definePrefs use()", () => {
  it("renders stored values and re-renders on write", async () => {
    await hydrateWith({ "tool:example": { dpi: 300 } });
    const group = makeGroup();

    const { result } = renderHook(() => group.use());
    expect(result.current[0].dpi).toBe(300);

    act(() => {
      result.current[1]("dpi", 450);
    });
    expect(result.current[0].dpi).toBe(450);
  });

  it("keeps the values object identity stable across re-renders", async () => {
    await hydrateWith({ "tool:example": { dpi: 300 } });
    const group = makeGroup();

    const { result, rerender } = renderHook(() => group.use());
    const first = result.current[0];
    rerender();

    // Load-bearing: a fresh object each render would loop any effect that
    // depends on these values.
    expect(result.current[0]).toBe(first);
  });

  it("isolates one group's writes from another's hook", async () => {
    await hydrateWith({});
    const a = makeGroup("tool:a");
    const b = makeGroup("tool:b");

    const hookB = renderHook(() => b.use());
    const before = hookB.result.current[0];
    act(() => {
      a.write("dpi", 500);
    });

    expect(hookB.result.current[0]).toBe(before);
  });
});
