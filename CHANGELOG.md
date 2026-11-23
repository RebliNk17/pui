# Changelog

Release notes use `## [X.Y.Z] - YYYY-MM-DD` headings matching `Cargo.toml` and
Git tags (`vX.Y.Z`). Only the selected version's section is published to GitLab.

## [Unreleased]

## [0.2.1] - 2026-11-23

### Fixed

- The update dialog renders release notes as Markdown (headings, lists, bold and code) instead of showing raw `###` and `**` markers.

## [0.2.0] - 2026-11-23

### Added

- Bundled Pi upgraded to 1.6.0; Settings → General shows the Pi version in use.
- Pui can keep its own Pi folder (providers, logins, models, defaults, skills, extensions and chats). On first launch it finds an existing `~/.pi/agent` and offers to move it in, copying settings and moving chats with a link left behind, so nothing is deleted. **Settings → General → Pi data** can copy settings again later.
- Pi only uses logins saved in Pui. Provider API keys from environment variables (for example a key exported as `OPENAI_API_KEY`) are no longer treated as logins; agent shell commands still receive them. An option re-enables environment keys.
- Settings no longer depend on the open chat: the model list, defaults for new chats (model and thinking effort) and the Auto classifier read Pi's configuration directly and refresh when providers change.
- Inline visuals are interactive: hover to scroll, click and type inside them; tall visuals show a scrollbar.
- Inline photos, screenshot thumbnails on activity rows, HTML file cards and Browser preview cards in chat, plus the `pui_visual` tool for inline HTML/SVG visuals.
- Auto classifier uses the same searchable, priced model picker as the composer, and is always visible in Settings → Permissions.
- New chats start in the last permission mode you chose (Auto the first time). Bypass is never remembered as a default.
- "Don't ask again" on the Bypass confirmation, with a reset in Settings → Permissions.
- Subagents can be started in the parent's mode or a stricter one, never a looser one.

### Changed

- Auto mode blocks more credential files (`.netrc`, `.npmrc`, `.kube`, `.docker`, `gh`, Pi's own config, and symlinks to them), catches disguised deletes (`/bin/rm`, `command rm`, `find -delete`, `git -C … push`, `sh -c "rm …"`), sends overwriting `cp`/`mv` to the classifier, and allows read-only `ls`/`git status`/`git log` without a classifier.
- The Auto classifier only uses models shown in the model picker, fails over to the next cheapest model when a provider errors, and reports the reason (for example "authentication failed").
- Settings → Permissions no longer has a mode dropdown; the mode is chosen per chat in the composer.
- The README is rewritten as a short guide with screenshots; the previous detailed text lives in `docs/reference.md`.

- Context breakdown now mirrors the full category list: Messages, MCP tools, System tools, System prompt, Skills, MCP server instructions, Memory files and Custom agents, plus the autocompact buffer, free space and deferred (registered but unloaded) MCP/system tools, with expandable per-tool, per-skill and per-file token lists.

### Fixed

- New providers appear in every chat within seconds, without restarting Pui, including in chats that are busy or in the background.
- Chats released after being idle can change model or effort again (they reconnect) instead of showing "Connect to Pi".
- The Background tasks card no longer widens the inspector, and durations stay clear of the scrollbar.
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
