import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import {
  getRasterFormats,
  rasterImageExtensions,
  __resetRasterFormatsCache,
  type RasterFormatDescriptor,
} from "./imageFormats";

const DESCRIPTORS: RasterFormatDescriptor[] = [
  { id: "png", name: "PNG", extensions: ["png"], supports_alpha: true },
  {
    id: "jpeg",
    name: "JPEG",
    extensions: ["jpg", "jpeg"],
    supports_alpha: false,
  },
  {
    id: "tiff",
    name: "TIFF",
    extensions: ["tif", "tiff"],
    supports_alpha: true,
  },
];

describe("imageFormats", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    __resetRasterFormatsCache();
  });

  it("getRasterFormats invokes the command and returns descriptors", async () => {
    invokeMock.mockResolvedValueOnce(DESCRIPTORS);

    const formats = await getRasterFormats();

    expect(invokeMock).toHaveBeenCalledWith("supported_raster_formats");
    expect(formats).toEqual(DESCRIPTORS);
  });

  it("memoizes the result across reads (one invoke for N calls)", async () => {
    invokeMock.mockResolvedValueOnce(DESCRIPTORS);

    const [a, b, c] = await Promise.all([
      getRasterFormats(),
      getRasterFormats(),
      getRasterFormats(),
    ]);

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(a).toBe(b);
    expect(b).toBe(c);
  });

  it("does not poison the cache on a failed fetch (next call retries)", async () => {
    invokeMock.mockRejectedValueOnce(new Error("ipc down"));
    await expect(getRasterFormats()).rejects.toThrow("ipc down");

    invokeMock.mockResolvedValueOnce(DESCRIPTORS);
    await expect(getRasterFormats()).resolves.toEqual(DESCRIPTORS);
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });

  it("rasterImageExtensions flattens every format's extensions in order", async () => {
    invokeMock.mockResolvedValueOnce(DESCRIPTORS);

    const exts = await rasterImageExtensions();

    expect(exts).toEqual(["png", "jpg", "jpeg", "tif", "tiff"]);
  });
});
