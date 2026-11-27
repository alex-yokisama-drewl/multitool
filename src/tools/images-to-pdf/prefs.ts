// Remembered options for this tool. Page size is a workflow preference
// ("everything I make is A4"); the staged file list is not — that belongs
// to one run and is deliberately left out.

import { definePrefs, oneOf } from "@/lib/prefs";
import type { PageSize } from "./types";

export const prefs = definePrefs(
  "tool:images-to-pdf",
  { pageSize: "auto-fit" },
  { pageSize: oneOf<PageSize>(["auto-fit", "a4", "letter"]) },
);
