import { useCallback, useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { generateParagraphs } from "./generator";

type CopyStatus = "idle" | "copied" | "failed";

const PARAGRAPH_COUNT = 5;
// "Copied" / "Copy failed" affordance hold time before snapping back to "Copy".
const COPY_AFFORDANCE_MS = 1500;

export function Lorem() {
  const navigate = useNavigate();
  const [text, setText] = useState<string>(() =>
    generateParagraphs(PARAGRAPH_COUNT),
  );
  const [copyStatus, setCopyStatus] = useState<CopyStatus>("idle");
  const copyResetRef = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (copyResetRef.current !== null) {
        window.clearTimeout(copyResetRef.current);
      }
    };
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") void navigate("/");
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [navigate]);

  const regenerate = useCallback(() => {
    setText(generateParagraphs(PARAGRAPH_COUNT));
  }, []);

  const copy = useCallback(async () => {
    if (copyResetRef.current !== null) {
      window.clearTimeout(copyResetRef.current);
    }
    try {
      await navigator.clipboard.writeText(text);
      setCopyStatus("copied");
    } catch {
      setCopyStatus("failed");
    }
    copyResetRef.current = window.setTimeout(() => {
      setCopyStatus("idle");
      copyResetRef.current = null;
    }, COPY_AFFORDANCE_MS);
  }, [text]);

  const paragraphs = text.split("\n\n");
  const copyLabel =
    copyStatus === "copied"
      ? "Copied"
      : copyStatus === "failed"
        ? "Copy failed"
        : "Copy";

  return (
    <div className="flex h-full flex-col gap-4">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Lorem Ipsum</h1>
        <div className="flex gap-2">
          <Button
            type="button"
            variant="outline"
            onClick={() => {
              void copy();
            }}
            aria-live="polite"
          >
            {copyLabel}
          </Button>
          <Button type="button" onClick={regenerate}>
            Regenerate
          </Button>
        </div>
      </div>
      <div
        data-testid="lorem-output"
        className="flex-1 overflow-auto rounded-lg border border-border bg-muted/30 p-4 text-sm leading-relaxed"
      >
        {paragraphs.map((p, i) => (
          <p key={i} className={i === paragraphs.length - 1 ? "" : "mb-4"}>
            {p}
          </p>
        ))}
      </div>
    </div>
  );
}
