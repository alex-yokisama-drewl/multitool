import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { pickAudioFile } from "@/lib/system";
import { fileName } from "@/lib/utils";

// Placeholder scaffold — the real UI (waveform, drag markers, MM:SS.ms
// numeric inputs, fade checkboxes, Preview, Trim) lands in commit 4.
// State machine mirrors the other tools but only `idle` and `picked`
// have real UX in this commit; `running` and `done` ship with the trim
// orchestrator wiring in commit 4 too.
type ViewState = { kind: "idle" } | { kind: "picked"; path: string };

export function AudioTrimmer() {
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

  const pick = async () => {
    const picked = await pickAudioFile();
    if (!picked) return;
    setState({ kind: "picked", path: picked });
  };

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-xl font-semibold">Audio Trimmer</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Trim an audio file to a range. Output preserves the source format.
        </p>
      </header>

      {state.kind === "idle" && (
        <Button onClick={() => void pick()}>Select audio file</Button>
      )}

      {state.kind === "picked" && (
        <div className="space-y-3">
          <div className="rounded-md border border-border bg-card px-3 py-2 text-sm font-mono">
            {fileName(state.path)}
          </div>
          <Button variant="outline" onClick={() => void pick()}>
            Pick different file
          </Button>
        </div>
      )}
    </div>
  );
}
