import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { allowImagePreview, pickImageFiles } from "@/lib/system";

// View state is split into `idle` and `staging` for E2. E3/E4 will widen
// `staging` to carry reorder + per-item remove, and add `running` / `done`
// / `error` for the convert flow. Keep the shape parallel to PdfToImages
// so the Create-PDF wiring in E4 lands without restructuring.
type ViewState = { kind: "idle" } | { kind: "staging"; paths: string[] };

function fileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}

/** Sort picked paths by filename ascending — the brief's "Initial order on
 * each pick batch" rule. Applied on every pick (initial + add-more) so the
 * user gets a stable starting point before any manual reorder. */
function sortByFilename(paths: string[]): string[] {
  return [...paths].sort((a, b) =>
    fileName(a).localeCompare(fileName(b), undefined, { sensitivity: "base" }),
  );
}

export function ImagesToPdf() {
  const navigate = useNavigate();
  const [state, setState] = useState<ViewState>({ kind: "idle" });

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") void navigate("/");
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [navigate]);

  const addImages = async () => {
    const picked = await pickImageFiles();
    if (!picked) return;
    // Grant per-path asset-protocol scope so E3 can render thumbnails via
    // `convertFileSrc()`. Doing it at pick-time keeps the staging view
    // ready for thumbnails without a separate later-IPC ceremony.
    await allowImagePreview(picked);
    setState((prev) => {
      const merged =
        prev.kind === "staging" ? [...prev.paths, ...picked] : picked;
      return { kind: "staging", paths: sortByFilename(merged) };
    });
  };

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-xl font-semibold">Images → PDF</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Assemble images into a single PDF, one image per page.
        </p>
      </header>

      {state.kind === "idle" && (
        <Button onClick={() => void addImages()}>Add images</Button>
      )}

      {state.kind === "staging" && (
        <div className="space-y-4">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">
              Staged ({state.paths.length})
            </div>
            <ul className="mt-2 space-y-1">
              {state.paths.map((path) => (
                <li key={path} className="break-all font-medium">
                  {fileName(path)}
                </li>
              ))}
            </ul>
          </div>
          <Button variant="outline" onClick={() => void addImages()}>
            Add more images
          </Button>
        </div>
      )}
    </div>
  );
}
