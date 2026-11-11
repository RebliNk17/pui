# Changelog

Release notes use `## [X.Y.Z] - YYYY-MM-DD` headings matching `Cargo.toml` and
Git tags (`vX.Y.Z`). Only the selected version's section is published to GitLab.

## [Unreleased]

## [0.1.0]

### Added

- Initial native Pui desktop workspace for the Pi coding agent.
- macOS Apple Silicon app packaging with the embedded Chromium browser and sandboxed helpers.
- GitLab CI validation, ZIP/DMG artifacts, SHA-256 checksums, and versioned GitLab Releases.
- Optional Developer ID signing and Apple notarization for protected release tags.

### Requirements and limitations

- Pi and Node.js are separate prerequisites; they are not installed by Pui.
- CI release assets currently support Apple Silicon only.
- Builds without Apple credentials are explicitly labeled ad-hoc and are not notarized.
- GitLab-based update checking and in-app changelog viewing are not enabled yet.
