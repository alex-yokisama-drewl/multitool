import type { ReactNode } from "react";
import { imageAssetUrl } from "@/lib/system";
import { fileName } from "@/lib/utils";

interface ImageThumbnailProps {
  /** Picked file path. Source for both `imageAssetUrl` and the rendered filename. */
  path: string;
  /**
   * When provided, renders an × button in the top-right corner that calls
   * back with the same `path`. Callers that don't want a remove affordance
   * (running / done views) just omit this.
   */
  onRemove?: (path: string) => void;
  /**
   * Optional content rendered under the filename — extra context the
   * specific tool wants to surface (e.g. predicted output name, source
   * size). Stays muted + small to keep the tile scannable.
   */
  subtitle?: ReactNode;
}

/**
 * Shared staging tile for image-pick lists. Renders a fixed-height image
 * preview via Tauri's asset protocol (`imageAssetUrl`), the basename
 * underneath, an optional subtitle, and an optional remove button.
 *
 * Used by the Image Format Converter and Image Compress tools. The
 * Images→PDF tool wraps the same visual in a drag-handle button so it
 * rolls its own structure — extracting that one too would mean shipping
 * the dnd-kit attributes through this component, which isn't worth the
 * coupling for one caller.
 */
export function ImageThumbnail({
  path,
  onRemove,
  subtitle,
}: ImageThumbnailProps) {
  const name = fileName(path);
  return (
    <li className="relative flex flex-col items-center gap-2 rounded-md border border-border bg-card p-2">
      <img
        src={imageAssetUrl(path)}
        alt=""
        draggable={false}
        className="h-24 w-full rounded object-contain"
      />
      <span className="line-clamp-2 w-full break-all text-center text-xs">
        {name}
      </span>
      {subtitle && (
        <div className="w-full text-center text-[11px] leading-tight text-muted-foreground">
          {subtitle}
        </div>
      )}
      {onRemove && (
        <button
          type="button"
          aria-label={`Remove ${name}`}
          onClick={() => onRemove(path)}
          className="absolute right-1 top-1 inline-flex h-6 w-6 items-center justify-center rounded-full border border-border bg-background text-sm leading-none shadow-sm hover:bg-accent"
        >
          ×
        </button>
      )}
    </li>
  );
}
