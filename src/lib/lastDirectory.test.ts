import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

import { hydratePrefs, resetPrefsForTest } from "./prefs";
import { directoryCandidates, rememberDirectory } from "./lastDirectory";

async function hydrateWith(entries: Record<string, unknown>) {
  mocks.invoke.mockImplementation((command: string) =>
    command === "load_settings"
      ? Promise.resolve({ version: 1, entries })
      : Promise.resolve(undefined),
  );
  await hydratePrefs();
}

beforeEach(() => {
  resetPrefsForTest();
  mocks.invoke.mockReset();
  mocks.invoke.mockResolvedValue(undefined);
});

describe("directoryCandidates", () => {
  it("offers nothing before anything has been picked", () => {
    expect(directoryCandidates("image")).toEqual([]);
  });

  it("prefers the kind's own directory over the global fallback", async () => {
    await hydrateWith({
      "app:last-directory": { image: "/home/a/photos", any: "/home/a/videos" },
    });
    expect(directoryCandidates("image")).toEqual([
      "/home/a/photos",
      "/home/a/videos",
    ]);
  });

  it("falls back to the last directory used anywhere for an unused kind", async () => {
    await hydrateWith({ "app:last-directory": { any: "/home/a/project" } });
    expect(directoryCandidates("video")).toEqual(["/home/a/project"]);
  });

  it("deduplicates when the kind directory is also the global one", async () => {
    await hydrateWith({
      "app:last-directory": { audio: "/home/a/music", any: "/home/a/music" },
    });
    expect(directoryCandidates("audio")).toEqual(["/home/a/music"]);
  });

  it("drops a value a corrupt document left as the wrong type", async () => {
    await hydrateWith({
      "app:last-directory": { image: 42, any: "/home/a/ok" },
    });
    expect(directoryCandidates("image")).toEqual(["/home/a/ok"]);
  });
});

describe("rememberDirectory", () => {
  it("records the picked file's directory under its kind and globally", async () => {
    await hydrateWith({});
    rememberDirectory("image", "/home/a/photos/shot.png");

    expect(directoryCandidates("image")).toEqual(["/home/a/photos"]);
    expect(directoryCandidates("video")).toEqual(["/home/a/photos"]);
  });

  it("keeps kinds independent", async () => {
    await hydrateWith({});
    rememberDirectory("image", "/home/a/photos/shot.png");
    rememberDirectory("video", "/home/a/clips/take.mp4");

    // The image memory survives a later video pick — that separation is
    // the whole point of keying on media kind.
    expect(directoryCandidates("image")).toEqual([
      "/home/a/photos",
      "/home/a/clips",
    ]);
    expect(directoryCandidates("video")).toEqual(["/home/a/clips"]);
  });

  it("overwrites the kind's previous directory", async () => {
    await hydrateWith({});
    rememberDirectory("pdf", "/home/a/one/x.pdf");
    rememberDirectory("pdf", "/home/a/two/y.pdf");

    expect(directoryCandidates("pdf")).toEqual(["/home/a/two"]);
  });

  it("handles Windows-style paths", async () => {
    await hydrateWith({});
    rememberDirectory("video", "C:\\Users\\a\\Videos\\take.mp4");

    expect(directoryCandidates("video")).toEqual(["C:\\Users\\a\\Videos"]);
  });

  it("ignores a bare filename with no directory part", async () => {
    await hydrateWith({});
    rememberDirectory("image", "shot.png");

    expect(directoryCandidates("image")).toEqual([]);
  });
});
