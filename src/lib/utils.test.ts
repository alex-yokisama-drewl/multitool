import { describe, expect, it } from "vitest";
import { parentDir } from "./utils";

describe("parentDir", () => {
  it("returns the directory of a POSIX path", () => {
    expect(parentDir("/home/a/photos/shot.png")).toBe("/home/a/photos");
  });

  it("returns the directory of a Windows path", () => {
    expect(parentDir("C:\\Users\\a\\Videos\\take.mp4")).toBe(
      "C:\\Users\\a\\Videos",
    );
  });

  it("keeps the root separator for a top-level file", () => {
    // "" would mean "unknown", so the root has to survive as "/".
    expect(parentDir("/notes.txt")).toBe("/");
  });

  it("returns empty for a bare filename", () => {
    expect(parentDir("shot.png")).toBe("");
    expect(parentDir("")).toBe("");
  });

  it("ignores trailing separators", () => {
    expect(parentDir("/home/a/photos/")).toBe("/home/a");
    expect(parentDir("/home/a/photos///")).toBe("/home/a");
  });

  it("handles a path with no extension", () => {
    expect(parentDir("/home/a/Makefile")).toBe("/home/a");
  });
});
