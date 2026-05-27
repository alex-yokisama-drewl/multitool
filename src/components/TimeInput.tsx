import { useState } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { formatMs, parseMs } from "@/lib/time";

export interface TimeInputProps {
  id: string;
  label: string;
  ms: number;
  max: number;
  onChange: (ms: number) => void;
  /// "right" right-aligns the input's text + label so an End block reads
  /// with its content flush against the row's right edge. Default is
  /// left-aligned (used by Start).
  align?: "left" | "right";
}

/// `MM:SS.mmm` numeric input shared by the media trimmers. Renders the
/// live value as a controlled string so partial edits ("00:02:") don't
/// get reformatted mid-keystroke; commits on blur (and Enter), at which
/// point the reformatted display lands.
export function TimeInput({
  id,
  label,
  ms,
  max,
  onChange,
  align = "left",
}: TimeInputProps) {
  const [text, setText] = useState(() => formatMs(ms));
  // Sync with the parent prop via a render-phase comparison rather than
  // `useEffect` — keeps `setText` out of an effect body (lint forbids it)
  // without a cascade. React converges on the next render when
  // `lastMs === ms`.
  const [lastMs, setLastMs] = useState(ms);
  if (ms !== lastMs) {
    setLastMs(ms);
    setText(formatMs(ms));
  }

  const commit = (raw: string) => {
    const parsed = parseMs(raw);
    if (parsed === null) {
      setText(formatMs(ms));
      return;
    }
    const clamped = Math.min(max, Math.max(0, parsed));
    onChange(clamped);
    setText(formatMs(clamped));
  };

  const alignClass = align === "right" ? "text-right" : "text-left";
  return (
    <div className={`space-y-1 ${align === "right" ? "text-right" : ""}`}>
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        type="text"
        inputMode="numeric"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={(e) => commit(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit(e.currentTarget.value);
        }}
        className={`font-mono w-32 ${alignClass}`}
      />
    </div>
  );
}
