# Changelog

All notable changes to this project follow [Semantic Versioning](https://semver.org/) and are sourced from Conventional Commit history.

## [0.2.1] - 2026-05-23

### Bug fixes

- **pdfium:** Bundle native binary as a Tauri resource (#12)

## [0.2.0] - 2026-05-23

### Bug fixes

- **rust:** Drop mobile crate-types so cargo test passes on Windows
- **rust:** Drop default-members so tauri build resolves the shell crate
- Remove malicious postinstall script from package.json (#7)
- **image-format-converter:** Reveal-folder + single-batch flow + thumbnails + retire planning docs (#9)

### CI

- Add cross-platform pipeline and release workflow
- Temp Windows diagnostic — dump test-exe imports on failure
- Bump checkout, setup-node, pnpm/action-setup from v4 to v6

### Chores

- Scaffold tauri 2 + react + ts template
- Configure frontend toolchain (tailwind, shadcn, zustand, router)
- Configure rust backend (error type, tokio, registry stubs)
- Add lefthook hooks and changelog config
- Add test harnesses (vitest, playwright, coverage)
- Lefthook clippy hook covers the workspace
- Deps
- Phase F generalization audit (three extractions + one principled skip) (#10)

### Documentation

- Add technical specification and working agreement
- Add scaffold plan and reference it from CLAUDE.md
- Mark scaffold plan phases A + B done
- Mark scaffold phase C done; add per-phase workflow rules
- Sync stale command references after the default-members revert
- Record configured branch protection in Phase G (#1)
- Scaffold readme, changelog, commands cheat sheet + doc reorg (#2)
- Retire SCAFFOLD_PLAN.md ahead of v0.1.0-scaffold tag (#3)
- Trim DECISIONS, retire per-tool brief, capture follow-ups (#5)

### Features

- PDF → Images tool (Phase 1 / first tool end-to-end) (#4)
- Add Images → PDF tool with reorderable staging (#6)
- Image Format Converter tool (#8)
- **dashboard:** Square color tiles + category grouping (#11)

### Refactoring

- **rust:** Split pure logic into multitool-core to unblock Windows CI

