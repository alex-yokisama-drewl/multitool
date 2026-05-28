# Tool: Lorem Ipsum Generator

## Summary
Generate 5 random lorem-ipsum paragraphs on demand; copy to clipboard or regenerate.

## Inputs
- None. No picker, no upload.

## Options
| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| _(none)_ | — | — | Intentionally configuration-free per backlog entry. |

## Output
- Location: clipboard (via Copy button) — no disk write.
- Naming: N/A.
- Duplicate handling: N/A — nothing touches the filesystem.

## UX flow
Dashboard → Lorem Ipsum tile → screen renders the 5 paragraphs in a scrollable read-only block with two buttons:
- **Copy** → `navigator.clipboard.writeText` on the rendered text; brief "Copied" affordance on the button itself. If `writeText` rejects, swap the affordance to "Copy failed" and leave the text on screen so the user can select-and-copy manually.
- **Regenerate** → produces a fresh random batch in place.

First mount auto-generates one batch so the screen is never empty. No progress, no cancellation, no error envelope — generation is synchronous and infallible.

## Architecture deviations from the standard tool pattern
- **No Rust side.** The tool is pure-frontend (`src/tools/lorem/` + a small generator under `src/lib/` or co-located). Generation is instant pure-string work — the heavy-work / file-I/O / cancellation justifications for the multitool-core + `#[tauri::command]` split don't apply. Per [ADDING_A_TOOL.md](../ADDING_A_TOOL.md) "What 'adding a tool' should look like": no edits to shared shell or routing code; the registry contract is honoured. Worth a one-line note in DECISIONS.md once the tool ships so future text-only tools (e.g. diff) follow the same shape.
- **No IPC wrapper.** Nothing to invoke; nothing to mock. The component holds the generator directly.
- **New `text` category.** Extend `ToolCategory` + `toolCategories` in [`src/tools/registry.ts`](../../src/tools/registry.ts) and update [`src/app/Dashboard.test.tsx`](../../src/app/Dashboard.test.tsx) to assert the new tile/section. This is the deliberate narrow shared edit ADDING_A_TOOL explicitly allows.

## Generation strategy
Hand-rolled, no new dependency.
- Lorem word corpus: ~70-word static array (the standard "lorem ipsum dolor sit amet…" vocabulary). Lives in `src/tools/lorem/generator.ts`.
- Pick random words; group into sentences of 6–14 words; first word capitalized, sentence ends with `.` (occasional `,` mid-sentence for cadence).
- Group sentences into paragraphs of 4–7 sentences.
- 5 paragraphs per batch, joined by blank lines.
- Uses `Math.random()` — placeholder text doesn't need cryptographic randomness, and `crypto.getRandomValues` is overkill for a UI generator.

Rationale for hand-rolling rather than pulling `lorem-ipsum` from npm: CLAUDE.md gates new deps on "a clear reason in the PR description"; ~40 lines of generator code with unit tests is cheaper than a transitive dependency for one tool, and lets the test suite assert exact distributional properties (paragraph count, sentence length range) without an external API to learn.

## Tile color
`teal` — already in the palette (no globals.css edit). Repeats with the Video Format Converter tile; categories are visually separated so the repeat doesn't collide.

## Clipboard approach
`navigator.clipboard.writeText` on the webview. No `tauri-plugin-clipboard-manager`, no new capability grant, no new `src/lib/system.ts` wrapper.

## Task plan

Commits build on each other; each row gets its SHA + a one-line surprise/gotcha note as it lands.

| # | Commit | Status | SHA | Note |
| - | --- | --- | --- | --- |
| 1 | `feat(lorem): scaffold tool + 'text' category` — extend `ToolCategory` + `toolCategories` in [`src/tools/registry.ts`](../../src/tools/registry.ts); add `src/tools/lorem/{index.ts,Lorem.tsx}` with placeholder component so the route renders; wire registry import; extend [`src/app/Dashboard.test.tsx`](../../src/app/Dashboard.test.tsx) for the new section + tile + color. | done | 9465661 | Dashboard.test.tsx has three describe blocks (tile-by-tile, grouping/order, color); each needed its own Text-section assertions. Router is registry-driven so no `router.tsx` edit. |
| 2 | `feat(lorem): paragraph generator + tests` — `src/tools/lorem/generator.ts` with the corpus + `generate(count)` returning a string; `generator.test.ts` asserting paragraph count, sentence-count range, word-count range, and that two calls produce different output. | done | 1e41bef | `splitSentences` had to special-case the trailing period since `split(". ")` only consumes the inner ". " separators. Ran the suite 5× consecutively — no flake on the "two calls differ" probabilistic check. |
| 3 | `feat(lorem): copy + regenerate UI` — replace the placeholder with the real `Lorem.tsx`: auto-generate on mount, paragraphs rendered in a read-only scrollable block, Copy uses `navigator.clipboard.writeText` with a transient "Copied" affordance, Regenerate produces a fresh batch. `Lorem.test.tsx` covers initial render, Regenerate updates text, Copy invokes the clipboard API, fallback message when `writeText` rejects. | done | ee7f146 | Two test-writing gotchas: (1) rendering paragraphs as separate `<p>` elements means `textContent` drops the `\n\n` separators, so the test reads `querySelectorAll("p")` and rejoins; (2) the `@typescript-eslint/unbound-method` rule rejects `expect(navigator.clipboard.writeText).toHaveBeenCalledWith(...)` — capture the `vi.fn()` into a local `writeText` const and assert on that. Dropped the originally-planned "affordance resets after timeout" test — fake timers don't flush the `writeText` microtask, and it's internal behaviour, not core acceptance. |
| 4 | **PAUSE for manual smoke session.** Specific scenarios listed below. | done | — | Smoke surfaced one gap: every other tool has Esc→dashboard but Lorem didn't. Folded the fix into the same commit as this row's update. |
| 4b | `feat(lorem): Esc-to-dashboard parity` — wire `useNavigate` + `useEffect` keydown listener for `Escape` matching the pattern in [`ImageCrop.tsx`](../../src/tools/image-crop/ImageCrop.tsx); wrap the existing `Lorem.test.tsx` renders in `MemoryRouter` so `useNavigate` resolves. No new test — Esc-on-Escape is uniform across tools and would be the third copy of the same assertion. | pending | — | — |
| 5 | `test(lorem): Playwright happy-path e2e` — `tests/e2e/lorem.spec.ts`: dashboard → Text section → Lorem tile → text visible → Regenerate changes text → Copy button reachable. Stub `navigator.clipboard.writeText` via `page.addInitScript` so the click resolves; no `src/lib/` wrapper to mock. | pending | — | — |
| 6 | `chore(lorem): ship` — DECISIONS entry ("Text-only tools skip the multitool-core split"), drop the Lorem backlog row, delete [LOREM.md](LOREM.md). | pending | — | — |

## Manual smoke checklist (commit 4 pause)
- Dashboard renders a new **Text** section with the Lorem tile in the configured color.
- Clicking the tile opens the tool screen with 5 paragraphs already rendered.
- Regenerate visibly changes the text.
- Copy puts the rendered text on the OS clipboard (paste somewhere external to confirm).
- Keyboard `Esc` returns to the dashboard (matches every other tool's pattern — confirm during smoke whether to add it; not in scope unless the user asks).
- No layout jump between Regenerate clicks.

## Edge cases
- **Clipboard permission denied.** If `writeText` rejects, surface a non-blocking inline error on the button ("Copy failed") and leave the text on screen so the user can select-and-copy manually. No toast plumbing yet in the codebase; inline is fine.
- **Component remount.** Each mount generates a fresh batch (no persistence across navigation away and back — matches "nothing else is needed").

## Acceptance
- [ ] Dashboard shows a new "Text" section with a single Lorem Ipsum tile.
- [ ] Clicking the tile opens the tool screen with 5 paragraphs already rendered.
- [ ] Regenerate produces a visibly different batch.
- [ ] Copy places the rendered text on the OS clipboard.
- [ ] Component renders consistently between batches (no layout jump).
- [ ] Generator unit tests: paragraph count = 5; each paragraph 4–7 sentences; each sentence 6–14 words; two consecutive calls produce different output.
- [ ] Component test: initial render shows non-empty text; Regenerate updates it; Copy invokes the clipboard wrapper.
- [ ] Playwright happy-path passes.
- [ ] All gates green: lint, typecheck, vitest, `pnpm tauri build --no-bundle`, Playwright.

---

_Status: plan committed; building commit 1._
