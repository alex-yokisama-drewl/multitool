// Shared progress UI for long-running tool jobs.
//
// Bundles the shadcn `<Progress>` bar, the "{label} N / total" status text,
// and the Cancel button so every tool's `running` view renders the same
// shape. The `label` prop is the noun the tool counts in (e.g. "page" for
// PDF → Images, "image" for Images → PDF); omit it for a bare "N / total".
//
// When `total === 0` (e.g. the job hasn't reported its first progress yet),
// the component renders a 0% bar and a "starting…" placeholder. This keeps
// Cancel available during the brief pre-first-event window — consumers
// don't have to special-case that themselves.

import { Button } from "@/components/ui/button";
import { Progress as ProgressBar } from "@/components/ui/progress";

export interface JobProgressProps {
  current: number;
  total: number;
  label?: string;
  onCancel: () => void;
}

export function JobProgress({
  current,
  total,
  label,
  onCancel,
}: JobProgressProps) {
  const known = total > 0;
  const percent = known ? (current / total) * 100 : 0;
  const prefix = label ? `${label} ` : "";
  const statusText = known ? `${prefix}${current} / ${total}` : "starting…";

  return (
    <div className="space-y-4">
      <ProgressBar value={percent} />
      <div className="text-sm text-muted-foreground">{statusText}</div>
      <Button variant="outline" onClick={onCancel}>
        Cancel
      </Button>
    </div>
  );
}
