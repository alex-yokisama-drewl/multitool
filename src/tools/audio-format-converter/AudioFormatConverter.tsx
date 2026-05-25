import { useEffect } from "react";
import { useNavigate } from "react-router-dom";

// Placeholder for the Audio Format Converter UI. Commit 1 ships the tile
// on the dashboard + the IPC wrappers; the real picker/form/progress UX
// lands in commit 8 once the Rust encode pipeline is in place.

export function AudioFormatConverter() {
  const navigate = useNavigate();

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") void navigate("/");
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [navigate]);

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-xl font-semibold">Audio Format Converter</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Convert one or more audio files to WAV, FLAC, MP3, or OGG Vorbis.
          Coming soon.
        </p>
      </header>
      <p className="text-sm text-muted-foreground">
        Implementation in progress &mdash; check back once the encoders land.
      </p>
    </div>
  );
}
