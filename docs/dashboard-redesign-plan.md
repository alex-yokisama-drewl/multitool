# Dashboard redesign — 0.2.0

> **Ephemeral working doc.** Deleted once the redesign ships. Tracks the commit-sized
> breakdown for the dashboard tile redesign listed in [../ASSIGNMENT.md §6](../ASSIGNMENT.md#6-roadmap)
> under 0.2.0. Architectural choices that emerge mid-build go to
> [../DECISIONS.md](../DECISIONS.md), not this file.

## Goal

Replace the current rectangular tool-list with square tiles, give each tile a soft
distinct color assigned at tool registration, and group tiles by file-type category
with a line + label separator between sections.

## Scope decisions (confirmed)

- **Color storage:** named CSS-variable tokens. Tools declare a token name (e.g.
  `"sky"`) in the registry; tokens resolve through `--tile-<name>` and `--tile-<name>-fg`
  defined in [../src/app/globals.css](../src/app/globals.css) with light + dark values.
  No Tailwind safelist needed.
- **Tile content:** name + description (current Tool type stays, no icon field added).
- **Category taxonomy:** by **file type the tool handles**, not by activity.
  - `pdf` → PDF → Images, Images → PDF
  - `image` → Image Format Converter
- **Empty categories:** not rendered. With the current registry only `pdf` and `image`
  sections show.
- **Category metadata:** lives in a small categories map alongside the registry
  (label + display order). Adding a category later means editing this map; adding a
  tool that fits an existing category still requires zero shared-code edits beyond
  the registry array entry (consistent with [adding-a-tool.md](adding-a-tool.md)).

## Out of scope

- Drag-and-drop input (a separate 0.2.0 line item — own plan when we get there).
- Output-location override UI (same — separate 0.2.0 line item).
- Icons on tiles (deliberately deferred — ARCHITECTURE.md's stale `icon` hint
  will be cleaned up when/if we add it later).

## Commit-sized breakdown

Each commit is independently buildable, gates green, and ships one focused concern.
They form a single PR.

### Commit 1 — `refactor(registry): retypify ToolCategory by file type`

**Why first:** purely data-shaped change with no UI impact. Lets the dashboard work
in commits 2–3 lean on the new shape.

**Touches**
- [../src/tools/registry.ts](../src/tools/registry.ts) — replace `ToolCategory` union
  with `"pdf" | "image"`; add `toolCategories` map exporting
  `{ id, label, order }` so the dashboard renders sections deterministically.
- [../src/tools/pdf-to-images/index.ts](../src/tools/pdf-to-images/index.ts) — `category: "pdf"`
- [../src/tools/images-to-pdf/index.ts](../src/tools/images-to-pdf/index.ts) — `category: "pdf"`
- [../src/tools/image-format-converter/index.ts](../src/tools/image-format-converter/index.ts) — `category: "image"`
- [../docs/adding-a-tool.md](adding-a-tool.md) — update the "Tool metadata" mention
  to reflect the file-type taxonomy + note that introducing a brand-new category
  requires extending `toolCategories` (the single allowed shared edit for that case).
- [../ARCHITECTURE.md](../ARCHITECTURE.md) — fix the stale comment listing
  `{ id, name, category, icon, route, component }` (drop `icon`, since we are
  not adding it; surface this contract truthfully).

**Tests**
- Existing [../src/app/Dashboard.test.tsx](../src/app/Dashboard.test.tsx) keeps
  passing untouched (no UI change yet).

**Gates:** `pnpm lint && pnpm typecheck && pnpm test` + Rust gates unaffected.

---

### Commit 2 — `feat(dashboard): group tiles by category with heading + separator`

**Why second:** purely structural UI change; preserves the existing tile shape so
visual regression is limited to the new headings.

**Touches**
- [../src/app/Dashboard.tsx](../src/app/Dashboard.tsx) — group tools by category;
  iterate `toolCategories` in declared order; render only non-empty sections;
  each section header is `<h2>` label + a horizontal rule (`<hr>` or a styled
  divider — divider lives in the same component, no shared change). Empty-state
  branch unchanged.
- [../src/app/Dashboard.test.tsx](../src/app/Dashboard.test.tsx) — add an
  assertion that the section headings render and the right tile sits under
  each. Existing per-tile assertions stay.

**Tests**
- Vitest: assert `getByRole('heading', { name: /pdf/i })` + that both PDF tools
  are under it (via `within(...)`).
- Playwright stays untouched — the existing happy-path specs navigate via the
  same link role, which is preserved.

**Gates:** `pnpm lint && pnpm typecheck && pnpm test && pnpm test:e2e`.

---

### Commit 3 — `feat(dashboard): square color tiles with per-tool tokens`

**Why last:** visual concern only; sits cleanly on top of the data + grouping
work from commits 1–2.

**Touches**
- [../src/app/globals.css](../src/app/globals.css) — add a small palette of soft
  tile color tokens. Pattern:
  ```css
  :root {
    --tile-sky:    oklch(0.94 0.04 230);
    --tile-sky-fg: oklch(0.30 0.10 230);
    --tile-amber:  oklch(0.95 0.06  85);
    --tile-amber-fg: oklch(0.32 0.10 85);
    /* … one per palette entry … */
  }
  .dark {
    --tile-sky:    oklch(0.30 0.06 230);
    --tile-sky-fg: oklch(0.92 0.04 230);
    /* … */
  }
  ```
  Palette sized to the current tool count + a few headroom slots (target ~6
  tokens — bikeshed during review). Tokens live next to the existing shadcn
  vars in the same file.
- [../src/tools/registry.ts](../src/tools/registry.ts) — extend `Tool` type with
  `color: TileColor` where `TileColor` is a string-literal union matching the
  palette tokens (compile-time safety against typos).
- Each tool's `index.ts` — set a distinct `color` (assign so neighbours within
  the same category are visually distinct, e.g. PDF→Images = `"rose"`,
  Images→PDF = `"amber"`, Image Format Converter = `"sky"`).
- [../src/app/Dashboard.tsx](../src/app/Dashboard.tsx) — tile rendering switches
  from rectangular list item to square card:
  - `aspect-square` on the link itself
  - background: `style={{ backgroundColor: 'var(--tile-' + color + ')', color: 'var(--tile-' + color + '-fg)' }}`
    (so Tailwind JIT doesn't need to know the token names ahead of time)
  - grid: `grid-cols-2 sm:grid-cols-3 md:grid-cols-4`
  - keep name (top) + description (below, `line-clamp-2` so long descriptions
    don't blow out the square)
- [../src/app/Dashboard.test.tsx](../src/app/Dashboard.test.tsx) — add a sanity
  assertion that the tile carries the expected inline style for its color
  token (or skip if too brittle and rely on the e2e screenshot).
- [../docs/adding-a-tool.md](adding-a-tool.md) — document the `color` field +
  point at the palette in `globals.css`.

**Tests**
- Vitest: tile link still navigates to `route` (unchanged), tile renders the
  color token in `style` (one focused assertion).
- Playwright: existing dashboard → tile → form flow still passes (link role is
  preserved).

**Gates:** `pnpm lint && pnpm typecheck && pnpm test && pnpm test:e2e` +
`pnpm tauri build --no-bundle`.

---

### Optional commit 4 — `docs: tick dashboard tile redesign on 0.2.0 roadmap`

Tiny doc-only commit that crosses the tile redesign off the 0.2.0 row in
[../ASSIGNMENT.md §6](../ASSIGNMENT.md#6-roadmap). Can be folded into commit 3
if we prefer; kept separate here for git-blame clarity.

## Open questions / risks

- **Color contrast in dark mode.** Soft pastels can lose contrast with light
  text. Mitigation: each token ships a paired `*-fg` foreground value tuned
  for AA contrast; verified visually during commit 3 review.
- **Description overflow.** Some descriptions are long; `line-clamp-2` may
  truncate awkwardly. If the truncation reads badly, fall back to a tooltip on
  hover (`title=` attribute is enough; no new shadcn primitive needed).
- **Tile order within a category.** Currently registration order. If a meaningful
  order emerges (alphabetical? recency?) it's a one-line change to add a sort —
  flag it in review.

## Done means

- All three current tiles render as soft-colored squares grouped under "PDF"
  and "Image" headings on the dashboard.
- Empty registry still shows the existing empty-state.
- Adding a new tool still requires only the two registry imports + the tool's
  own folder (and now picking a `color` from the palette).
- This file is deleted in the merging PR.
