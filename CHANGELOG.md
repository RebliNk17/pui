# Changelog

Release notes use `## [X.Y.Z] - YYYY-MM-DD` headings matching `Cargo.toml` and
Git tags (`vX.Y.Z`). Only the selected version's section is published to GitLab.

## [Unreleased]

## [0.1.0] - 2026-11-16

### Added

- Initial native Pui desktop workspace for the Pi coding agent, with persistent chats, model controls, permissions, managed subagents and scheduled tasks.
- Native Git status/history/review and guarded index actions, workspace file search/previews, table tools, terminal panes and context capture.
- Resource management, optional Pui Engineering skills, appearance/accessibility controls, and shared multi-window session ownership.
- macOS Apple Silicon app packaging with the embedded Chromium browser and sandboxed helpers.
- Single-download runtime: Pi 1.5.2, Node.js 22.23.5 with npm/npx, fd and ripgrep; pinned checksums/npm integrity, license inventories, and offline relocated-app smoke checks.
- CircleCI macOS validation/builds and protected-tag GitLab Releases with ZIP/DMG downloads, SHA-256 checksums, runtime metadata and changelog notes; isolated publication credentials and server-verified tag provenance.
- Optional Developer ID signing and Apple notarization for protected release tags.

### Requirements and limitations

- Pi and Node.js are included in the macOS app. Model-provider login/API keys are still required; user extensions and external tools may have their own prerequisites.
- CI release assets currently support Apple Silicon only.
- Builds without Apple credentials are explicitly labeled ad-hoc and are not notarized.
- GitLab-based update checking and in-app changelog viewing are not enabled yet.
