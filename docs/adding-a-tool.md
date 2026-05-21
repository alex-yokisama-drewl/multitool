# Multitool — Adding a Tool

> **Working doc — thin draft.** This is the playbook for adding a new tool to the registry. It is intentionally light: most "shared surfaces" don't exist yet because no tool has been built. **Revise this guide once PDF→Images ships** — at that point real helpers exist and the TODOs at the bottom should be resolved. The conceptual patterns these steps embody live in [../ARCHITECTURE.md](../ARCHITECTURE.md); per-tool product briefs live in [tools/](tools/).

## When to read this

Read this when adding a new tool to the registry. For a single tool, also read its per-tool brief in `docs/tools/<tool-id>.md` (write one first if it doesn't exist — template at the bottom).

## What "adding a tool" should look like

A new tool is meant to be **two new folders + two import lines + tests**, with no edits to shared shell or routing code. If you find yourself editing a shared file for anything other than the two registry imports, stop — that's a sign something belongs in a shared surface (and merits an entry in [../DECISIONS.md](../DECISIONS.md) explaining why).

## Steps

### 1. Read the per-tool brief
- `docs/tools/<tool-id>.md` — inputs, options, output naming, edge cases, acceptance
- If missing, write it first using the template at the bottom of this file

### 2. Create the Rust modules
A tool spans two folders: a pure-logic module in `multitool-core` and a thin Tauri-aware wrapper in the shell.
- **In `multitool-core`** (`multitool-core/src/tools/<tool_id>/`):
  - `convert.rs` (and any other pure logic) — `fn convert(input, opts, …) -> Result<output, AppError>`. No tauri imports. Pure logic always lives here regardless of dep weight — see [../DECISIONS.md](../DECISIONS.md) → "Heavy deps allowed in `multitool-core`".
- **In the Tauri shell** (`src-tauri/src/tools/<tool_id>/`):
  - `mod.rs` — `#[tauri::command]` entry points; wraps the pure function with progress events, cancellation, and result envelope. This is the only place Tauri-aware code lives.
- For multi-output tools (1 input → N outputs), use the streaming `on_page` callback pattern — see [../DECISIONS.md](../DECISIONS.md) → "Streaming `on_page` callback".
- Tests:
  - Unit tests on `convert.rs` covering options, edge cases, and every error path (target ≥80% line cov — see [../ARCHITECTURE.md §4](../ARCHITECTURE.md#4-testing-strategy))
  - Integration test on the shell command only if the IPC envelope has non-trivial logic (event ordering, JobRegistry interaction, etc.)

### 3. Register the Rust commands
- Add `pub mod <tool_id>;` in [../src-tauri/src/tools/mod.rs](../src-tauri/src/tools/mod.rs)
- Append the tool's commands to the `generate_handler!` call in `register_commands`
- That should be the only edit to a shared Rust file

### 4. Create the frontend module
- Folder: `src/tools/<tool-id>/`
- Files:
  - `index.ts` — exports `Tool` metadata: `{ id, name, description, category, route, component }`
  - `<ToolName>.tsx` — presentational; all IPC goes through wrappers in `src/lib/`
  - `types.ts` — TS mirrors of Rust `Opts` / result types
- Use primitives from `src/components/` and shadcn components in `src/components/ui/`. Don't reach into another tool's folder.

### 5. Register the frontend tool
- Add the import + entry in [../src/tools/registry.ts](../src/tools/registry.ts)
- That should be the only edit to a shared frontend file

### 6. Wire IPC through `src/lib/`
- All `@tauri-apps/api` calls live in [../src/lib/](../src/lib/) wrappers
- Tool components import the wrapper, never `@tauri-apps/api` directly
- This is the seam Playwright mocks for e2e (see [../ARCHITECTURE.md §4](../ARCHITECTURE.md#4-testing-strategy))

### 7. Add tests
| Layer | Runner | What to cover |
| --- | --- | --- |
| Rust pure | `cargo test -p multitool-core` | Inputs, options, edge cases, every error variant |
| Rust shell glue | `cargo test` against the shell | Only if the IPC envelope has non-trivial logic |
| React component | Vitest + Testing Library | Renders; calls IPC wrapper; surfaces errors |
| E2E | Playwright against `pnpm dev` | Happy path: pick file → convert → see result |

### 8. Run the pre-PR checklist
See [../CLAUDE.md → Per-PR checklist](../CLAUDE.md). Short version: fmt → clippy (workspace) → `cargo test -p multitool-core` → pnpm lint/typecheck/test → `pnpm tauri build --no-bundle`.

## Shared surfaces

A new tool should consume from these, not duplicate. Most are stubs today — they fill in as tools land.

| Surface | Location | Status |
| --- | --- | --- |
| Tauri IPC wrappers | [../src/lib/](../src/lib/) | **TODO** — only `cn` util exists; first IPC wrapper lands with PDF→Images |
| shadcn UI primitives | [../src/components/ui/](../src/components/ui/) | **TODO** — empty; generate on demand with `pnpm dlx shadcn add <component>` |
| Shared components (FilePicker, JobProgress, …) | [../src/components/](../src/components/) | **TODO** — none yet; extract from PDF→Images |
| Path + duplicate-name helpers (pure) | `multitool-core/src/fs.rs` *(lands with PDF→Images)* | **TODO** — `unique_path` and the output writer land in `multitool-core` per [../DECISIONS.md](../DECISIONS.md) → "Heavy deps allowed in `multitool-core`" |
| Tauri-aware path helpers | [../src-tauri/src/fs/](../src-tauri/src/fs/) | **TODO** — module stub only; reserved for resolution that genuinely needs Tauri APIs (e.g. app-data dir) |
| Error type | `multitool_core::error::AppError` | Available — variants: `FileNotFound`, `PermissionDenied`, `UnsupportedFormat`, `ProcessingFailed`, `Cancelled` |
| Cancellation | `multitool_core::ipc::JobRegistry` + `cancel_job` command | Available |

When a tool needs something not in this list, decide: (a) one-off inside the tool's folder, or (b) extract to a shared surface. Bias toward (a) until you see the pattern twice — see [../CLAUDE.md → Scope discipline](../CLAUDE.md).

## Per-tool brief template

Save as `docs/tools/<tool-id>.md`. Keep it short and implementation-flavored — the PM-level scope still lives in [../ASSIGNMENT.md](../ASSIGNMENT.md).

```markdown
# Tool: <Name>

## Summary
One sentence.

## Inputs
- ...

## Options
| Option | Type | Default | Notes |
| --- | --- | --- | --- |

## Output
- Location:
- Naming:
- Duplicate handling: per [../../ARCHITECTURE.md §3.3](../../ARCHITECTURE.md#33-file-io-conventions)

## UX flow
Dashboard → <Tool> → Result. Note non-default flows + cancellation behavior.

## Edge cases
- ...

## Acceptance
- [ ] ...
```

## TODOs — revisit after PDF→Images ships

This guide was written before any tool exists, so several sections are aspirational. After PDF→Images lands, revisit:

- [ ] Replace each "Shared surfaces" TODO with concrete helper names + signatures
- [ ] Add the first IPC wrapper pattern (event subscription + cancellation) as a worked example
- [ ] Confirm the "pure logic in `multitool-core`" rule held up in practice (if not, update DECISIONS)
- [ ] Decide whether progress events have a shared shape worth documenting once and for all
- [ ] Trim or rewrite anything here that didn't survive contact with the first real tool
- [ ] Decide whether the per-tool brief template needs more / fewer sections
- [ ] Decide whether the "ephemeral build-plan file" pattern (e.g. `docs/tools/<tool-id>-plan.md`) earned its keep, and either bake it into the playbook or drop it
