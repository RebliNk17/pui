# Changelog

Release notes use `## [X.Y.Z] - YYYY-MM-DD` headings matching `Cargo.toml` and
Git tags (`vX.Y.Z`). Only the selected version's section is published to GitLab.

## [Unreleased]

### Added

- Context breakdown now mirrors the full category list: Messages, MCP tools, System tools, System prompt, Skills, MCP server instructions, Memory files and Custom agents, plus the autocompact buffer, free space and deferred (registered but unloaded) MCP/system tools, with expandable per-tool, per-skill and per-file token lists.

### Fixed

- Antigravity now persists each account's live model catalog, so Gemini 4 Neon/Argon/Hydrogen, orchestrators and other entitled models are listed at startup (and after a failed refresh) instead of only the five built-in fallback models.

## [0.1.1] - 2026-11-19

### Added

- Multi-account provider support with automatic quota fallback (`accounts.ts`) and live Antigravity model catalog refresh (`refreshAntigravityModels`).
- Persistent usage cache across app relaunches (`CachedUsage`) and grouped Antigravity quota summary (`retrieveUserQuotaSummary`).

### Fixed

- Antigravity `agy` CLI endpoint/User-Agent compatibility so Gemini 4 Neon, Argon, Hydrogen, and Orchestrator models and quota groups load properly.
- Avoided HTTP 429 false-positives by sanitizing Pi harness markers from `systemInstruction` and distinguishing model capacity exhaustion from quota exhaustion.
- Reserved scrollbar gutter margin in the usage popover so the scrollbar no longer overlaps countdown text.

## [0.1.0] - 2026-11-18

### Added

- Initial native Pui desktop workspace for the Pi coding agent, with persistent chats, model controls, permissions, managed subagents and scheduled tasks.
- Native Git status/history/review and guarded index actions, workspace file search/previews, table tools, terminal panes and context capture.
- Resource management, optional Pui Engineering skills, appearance/accessibility controls, and shared multi-window session ownership.
- macOS Apple Silicon app packaging with the embedded Chromium browser and sandboxed helpers.
- Single-download runtime: Pi 1.5.2, Node.js 22.23.5 with npm/npx, fd and ripgrep; pinned checksums/npm integrity, license inventories, and offline relocated-app smoke checks.
- GitHub Actions macOS validation/builds and tagged GitHub Releases with ZIP/DMG downloads, SHA-256 checksums, runtime metadata and changelog notes.
- Optional Developer ID signing and Apple notarization for protected release tags.

### Requirements and limitations

- Pi and Node.js are included in the macOS app. Model-provider login/API keys are still required; user extensions and external tools may have their own prerequisites.
- CI release assets currently support Apple Silicon only.
- Builds without Apple credentials are explicitly labeled ad-hoc and are not notarized.
- GitHub-based automatic/manual update checking, verified ZIP download (`SHA256SUMS` + `codesign`), in-app changelog viewing, background install-on-quit relauncher, and automatic temporary file cleanup.
