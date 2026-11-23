# Mobile Remote Access & Android Companion Validation

## Summary

Implemented end-to-end mobile remote access for Pui (Phase 1: Tailscale direct HTTP + WebSocket; Phase 2-ready swappable transport for an E2EE Cloud Relay) plus the cross-platform Expo/React Native companion app (`mobile/`, Android now and iOS ready).

### Host side (`src/remote/`, `src/app/remote_host.rs`, `src/app/settings/page.rs`, `src/app/sidebar.rs`, `src/sessions.rs`)
- Pure-Rust HTTP/1.1 + RFC 6455 WebSocket server with zero new Cargo dependencies (`Cargo.lock` unchanged; `--offline --locked` builds preserved).
- Tailscale IPv4 (`100.64.0.0/10`) auto-detection, constant-time bearer token auth, `pui://pair?...` QR pairing URI, and pure-Rust QR code matrix renderer inside **Settings → Remote & Mobile**.
- Durable `SessionOrigin` (`desktop`, `android`, `ios`, `web`, `cli` + device name) persisted in `ChatPrefs` and Pi session JSONL (`pui.session-origin.v1`) and surfaced as a subtle origin icon/tooltip in the desktop sidebar and mobile session list.
- Endpoints for live sessions, message streaming, creating sessions with origin, sending prompts/images/slash commands, aborting, answering tool approvals & `pui_ask_questions`, switching permission modes/models, `/` slash command completions, `@` workspace file mention completions, canonicalized workspace file tree/content/raw artifact serving, and Git changes/diffs.

### Mobile app (`mobile/`)
- Cross-platform Expo / React Native + TypeScript app (`dev.pi.pui.mobile` for Android & iOS).
- QR code & manual Tailscale host pairing with `expo-secure-store` token storage and configurable device name.
- Sessions screen with origin badges, filters (`All`, `From phone`, `Running`, `Needs you`), and New Session sheet.
- Chat screen with streaming messages, collapsible thinking & tool activity, inline tool approvals & `pui_ask_questions` flow, image attachments, permission mode & model pickers, and cursor-aware `/` slash command + `@` file mention autocomplete popups.
- Dedicated Artifacts tab, workspace File Tree + code/image/sandboxed HTML viewer (strict CSP WebView, no full browser chrome), and Git Changes/Diff viewer.

### CI & Releases (`.github/workflows/android.yml`, `.github/workflows/macos.yml`)
- Added path-filtered Android CI workflow on `ubuntu-latest` that runs `npm ci`, `npm run typecheck`, `npm test`, `expo prebuild --platform android`, and Gradle `assembleRelease`, attaching `pui-android.apk` + `pui-android.apk.sha256` on `vX.Y.Z` release tags.

## Validation Performed

- `bash scripts/test-rust.sh` — **passed** (all bulk + serial Rust test groups, including `remote::*`, `app::remote_host::tests::*`, `app::settings::page::design_tests::*` across dark/light themes & viewports, and `tests/prompt_design.rs`).
- `cargo fmt -- --check` — **passed**.
- `cd mobile && npm run typecheck` (`tsc --noEmit`) — **passed** (0 errors).
- `cd mobile && npm test` — **passed** (14/14 unit tests across pairing, `/` & `@` completions, CSP HTML sandbox, and approval/`pui_ask_questions` dialog parsers).
- `cd mobile && npx expo config --type public` — **passed** (validated `app.config.ts` and Android cleartext Tailscale config plugin).
