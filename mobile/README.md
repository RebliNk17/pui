# Pui Mobile (Android now · iOS ready)

Cross-platform Expo / React Native companion app for [Pui](../README.md). Connects to the Pui Desktop host running on your computer over **Tailscale** (Phase 1) and is architected with a swappable `PuiTransport` interface for an end-to-end encrypted **Cloud Relay** (Phase 2).

## Features

- **Tailscale QR pairing & multi-host support**: Scan the QR code in Pui Desktop → **Settings → Remote & Mobile**, or enter your Tailscale IP (`100.x.y.z`) and bearer token manually. Tokens are stored in Android Keystore / iOS Keychain via `expo-secure-store`.
- **Full live communication**: Real-time WebSocket updates, send/steer/queue messages, abort runs, attach images, switch permission modes (`Auto`, `Manual`, `Accept edits`, `Plan`, `Don't ask`, `Bypass`) and models.
- **Slash commands (`/`) & `@` file autocomplete**: Cursor-aware completion popups matching Pui Desktop (`/` for Pui + Pi session commands; `@` for live workspace file search).
- **Tool approvals & `pui_ask_questions`**: Approve or deny tool calls and answer multi-step agent questions directly from your phone.
- **Session origin tracking**: Sessions created from Android/iOS are tagged with `{ platform, device, client }` so both Pui Desktop's sidebar and the phone's session list show where each session started.
- **Artifacts, HTML, Images & File Tree (no full browser)**:
  - Native workspace file tree & code viewer
  - Native full-screen image viewer
  - Sandboxed inline & fullscreen WebView with strict CSP for `pui_visual` cards and `.html` artifacts
  - Git branch status & color-coded unified diffs

## Quick Start (Tailscale)

1. Install [Tailscale](https://tailscale.com) on your Mac (running Pui) and on your Android phone, signed into the same tailnet.
2. In **Pui Desktop**, open **Settings → Remote & Mobile** and turn on **Enable remote access** (default bind mode: *Tailscale only* on port `4817`).
3. Install the **Pui Android APK** from [GitHub Releases](https://github.com/RebliNk17/pui/releases) (or build locally below).
4. Open Pui Mobile, tap **Pair with Pui Desktop**, and scan the QR code.

## Local Development

```bash
cd mobile
npm install
npm run typecheck
npm test
npx expo run:android    # Android device / emulator
npx expo run:ios        # Future iOS build on macOS with Xcode
```

## Protocol & Roadmap

See [`docs/mobile-remote-protocol.md`](../docs/mobile-remote-protocol.md) for the versioned `v1` HTTP + WebSocket specification and Phase 2 E2EE Cloud Relay plan.
