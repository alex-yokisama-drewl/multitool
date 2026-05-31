import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { imageAssetUrlMock } = vi.hoisted(() => ({
  imageAssetUrlMock: vi.fn((p: string) => `asset://${p}`),
}));

vi.mock("@/lib/system", () => ({
  imageAssetUrl: imageAssetUrlMock,
}));

import { ImageThumbnail } from "./ImageThumbnail";

const PATH = "/tmp/photo.png";

beforeEach(() => {
  imageAssetUrlMock.mockClear();
});

describe("ImageThumbnail", () => {
  it("renders the filename and asset-resolved image src", () => {
    const { container } = render(
      <ul>
        <ImageThumbnail path={PATH} />
      </ul>,
    );
    expect(screen.getByText("photo.png")).toBeInTheDocument();
    // alt="" makes <img> presentational (no implicit role), so go through
    // the DOM directly.
    const img = container.querySelector("img");
    expect(img).toHaveAttribute("src", `asset://${PATH}`);
  });

  it("omits the remove button when onRemove is not supplied", () => {
    render(
      <ul>
        <ImageThumbnail path={PATH} />
      </ul>,
    );
    expect(screen.queryByRole("button", { name: /remove/i })).toBeNull();
  });

  it("renders a remove × button and calls back with the path when onRemove is supplied", () => {
    const onRemove = vi.fn();
    render(
      <ul>
        <ImageThumbnail path={PATH} onRemove={onRemove} />
      </ul>,
    );
    const remove = screen.getByRole("button", { name: /remove photo\.png/i });
    fireEvent.click(remove);
    expect(onRemove).toHaveBeenCalledWith(PATH);
  });

  it("renders the optional subtitle slot under the filename", () => {
    render(
      <ul>
        <ImageThumbnail
          path={PATH}
          subtitle={<span>→ photo_compressed.webp</span>}
        />
      </ul>,
    );
    expect(screen.getByText("→ photo_compressed.webp")).toBeInTheDocument();
  });
});
