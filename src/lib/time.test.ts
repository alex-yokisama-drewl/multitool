import { describe, expect, it } from "vitest";
import { formatMs, parseMs } from "./time";

describe("formatMs / parseMs", () => {
  it("formatMs zero-pads minutes / seconds / millis", () => {
    expect(formatMs(0)).toBe("00:00.000");
    expect(formatMs(1_500)).toBe("00:01.500");
    expect(formatMs(75_005)).toBe("01:15.005");
  });

  it("formatMs clamps negatives to zero", () => {
    expect(formatMs(-1)).toBe("00:00.000");
  });

  it("parseMs round-trips with formatMs", () => {
    for (const ms of [0, 1_500, 75_005, 359_999]) {
      expect(parseMs(formatMs(ms))).toBe(ms);
    }
  });

  it("parseMs accepts MM:SS without milliseconds", () => {
    expect(parseMs("01:30")).toBe(90_000);
  });

  it("parseMs rejects seconds >= 60", () => {
    expect(parseMs("00:60.000")).toBeNull();
  });

  it("parseMs rejects garbage", () => {
    expect(parseMs("hello")).toBeNull();
    expect(parseMs("1:2:3")).toBeNull();
    expect(parseMs("")).toBeNull();
  });
});
