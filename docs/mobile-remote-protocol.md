# Pui Remote & Mobile Protocol (`v1`)

This document specifies the host-to-mobile protocol exposed by Pui (`src/remote/`) and consumed by the Pui Mobile app (`mobile/`).

## Design Goals

1. **Same protocol, swappable transport.**
   - **Phase 1 (Tailscale / LAN):** Phone connects directly to Pui over Tailscale (`ws://<tailscale-ip>:4817/v1/ws` + HTTP REST) or via `tailscale serve` (`wss://` / `https://`).
   - **Phase 2 (Cloud Relay):** Pui establishes an outbound WSS connection to a blind cloud relay and wraps the exact same `v1` JSON frames in end-to-end encryption (X25519 + ChaCha20-Poly1305). Mobile switches from `DirectTransport` to `CloudRelayTransport` without changing UI or state logic.
2. **Single Pi writer.** Pui Desktop remains the authoritative supervisor for Pi RPC processes. Desktop and Mobile observe the same live sessions and send actions through Pui.
3. **Explicit session origin.** Every session carries a durable `origin` (`desktop`, `android`, `ios`, `web`, `cli`) plus optional `device` and `client` names so both Desktop and Phone clearly show where each conversation started.
4. **Rich mobile artifacts without a full browser.** File trees, code previews, Git diffs, and images render natively; `pui_visual` inline HTML/CSS/SVG cards and HTML files render in a sandboxed WebView with a restrictive CSP (`default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:`).

---

## Pairing & Authentication

### QR Pairing URI

Pui Settings → **Remote & Mobile** renders a QR code encoding:

```text
pui://pair?v=1&host=<tailscale-ip-or-hostname>&port=4817&token=<hex-token>&name=<url-encoded-host-name>&tls=0
```

- `v`: protocol version (`1`).
- `host`: detected Tailscale IPv4 (`100.64.0.0/10`) or configured hostname.
- `port`: listening TCP port (default `4817`).
- `token`: 256-bit random hex bearer token stored in Pui preferences (and in Android Keystore / iOS Keychain on mobile).
- `tls`: `1` when served behind `tailscale serve` / HTTPS reverse proxy, `0` for direct Tailscale WireGuard tunnel.

### Request Authentication

All endpoints except `GET /v1/health` require authentication via either:

- `Authorization: Bearer <token>` header (preferred for REST and WS clients that support custom headers), or
- `?token=<token>` query parameter (for WebSocket upgrades and media/image URLs).

Tokens are verified with constant-time byte comparison. Unauthenticated requests receive `401 Unauthorized`.

---

## Data Types

### `SessionOrigin`

```json
{
  "platform": "android",
  "device": "Pixel 9",
  "client": "pui-mobile",
  "created_at": 1763913600000
}
```

`platform` is one of: `"desktop"`, `"android"`, `"ios"`, `"web"`, `"cli"`.

### `ArtifactItem`

Extracted from session messages (`pui_visual` inline HTML/SVG cards, `pui_open` targets, and image attachments):

```json
{
  "id": "visual-0-1",
  "kind": "visual_html | file_html | image | file",
  "title": "Architecture Diagram",
  "html": "<!doctype html>...",
  "height": 380,
  "path": "/path/to/artifact.html",
  "mime": "image/png",
  "data_base64": "..."
}
```

---

## HTTP REST Endpoints

| Method & Path | Purpose |
|---|---|
| `GET /v1/health` | Unauthenticated liveness check (`{ ok, protocol_version, pui_version }`). |
| `GET /v1/info` | Host metadata, Tailscale IP, known projects, permission modes, active session path. |
| `GET /v1/sessions` | All known and live sessions with `origin`, `status`, `busy`, `has_pending_dialog`, `unread`, `project`, `cwd`, `model`, `modified`. |
| `GET /v1/sessions/detail?path=<encoded>` | Full session detail: `messages`, `pending_messages`, `dialogs`, `commands`, `models`, `model`, `provider`, `thinking`, `levels`, `permission_mode`, `cost`, `context_tokens`, `context_window`, `busy`, `origin`, `artifacts`. |
| `POST /v1/sessions` | Create a new session in `{ cwd, permission_mode?, prompt?, origin }`. Returns `{ ok, request_id, origin }`. |
| `POST /v1/sessions/open` | Focus/activate a saved session on host (`{ path }`) so Pi loads its live state. |
| `POST /v1/sessions/message` | Send a prompt, steer, queue, or slash command (`{ path?, text, mode?, images?, origin? }`). |
| `POST /v1/sessions/abort` | Abort the active turn and clear queued messages (`{ path? }`). |
| `POST /v1/sessions/dialog` | Respond to a pending tool approval, `pui_ask_questions`, confirm, select, or input dialog (`{ path?, dialog_id, response }`). |
| `POST /v1/sessions/settings` | Update session `{ path?, permission_mode?, model?, provider?, thinking? }`. |
| `GET /v1/completions/commands?path=<encoded>&q=<query>` | Slash command completions combining Pui local commands (`/new`, `/compact`, `/reload`, `/stop`, `/model`, `/files`, `/help`) and Pi's session commands (`state.commands`). |
| `GET /v1/completions/files?cwd=<encoded>&q=<query>` | `@` file/path completions backed by Pui's `files::mention_paths` matcher. |
| `GET /v1/files/tree?cwd=<encoded>&dir=<rel>` | Workspace-scoped directory listing (sorted directories first, bounded entry count). |
| `GET /v1/files/content?cwd=<encoded>&path=<rel>` | UTF-8 text/code file content (up to 512 KiB) with `is_html`, `is_image`, `size` metadata. |
| `GET /v1/files/raw?cwd=<encoded>&path=<path>` | Raw file bytes with MIME type for images, SVGs, and HTML artifacts. Strictly confined to allowed workspace roots or explicitly opened session artifacts. |
| `GET /v1/git/changes?cwd=<encoded>` | Git repository status (`branch`, `is_git`, modified `files` with added/removed counts). |
| `GET /v1/git/diff?cwd=<encoded>&path=<rel>` | Unified Git diff for a single relative workspace file path. |

---

## Real-Time WebSocket (`GET /v1/ws`)

After upgrading over RFC 6455, the server pushes JSON envelopes whenever state changes:

```json
{ "type": "hello", "protocol_version": 1, "host_name": "Shai's Mac", "active_path": "..." }
{ "type": "sessions_updated", "revision": 42, "sessions": [ ... ] }
{ "type": "session_updated", "revision": 43, "path": "...", "detail": { ... } }
{ "type": "pong", "ts": 1763913600000 }
```

Clients may send JSON frames:

```json
{ "type": "ping", "ts": 1763913600000 }
{ "type": "subscribe", "path": "/path/to/session.jsonl" }
{ "type": "client_hello", "platform": "android", "device": "Pixel 9", "client": "pui-mobile" }
```

## Security Model

- **Default off:** Remote access is disabled until explicitly enabled in **Settings → Remote & Mobile**.
- **Tailscale-first binding:** Default bind mode attaches only to the detected Tailscale IPv4 address (`100.64.0.0/10`), never exposing the server to public Wi-Fi unless the user explicitly switches to "All interfaces".
- **Canonical path confinement:** Every file tree, file preview, raw artifact, and diff request canonicalizes paths and rejects `..` traversal or reads outside configured project folders and explicitly recorded session artifacts.
- **Sandboxed HTML previews:** Mobile renders `pui_visual` cards and `.html` artifacts in an isolated WebView with opaque origin and restrictive CSP—no cookies, no network requests, no form submission, no local file access.
