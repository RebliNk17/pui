# Changelog

Release notes use `## [X.Y.Z] - YYYY-MM-DD` headings matching `Cargo.toml` and
Git tags (`vX.Y.Z`). Only the selected version's section is published to GitLab.

## [Unreleased]

## [1.1.0] - 2026-11-24

### Added

- **Cloud agents:** a headless Pui cloud daemon (`daemon/`) runs agents 24/7 on your own Linux host over Tailscale, with an [Oracle Cloud Always Free setup guide](docs/cloud-host-oracle.md).
- Start cloud chats from Desktop **New Chat** or the phone; each agent works in an isolated `pui_agent/<slug>-<id>` worktree under Auto mode and can commit, push and open a PR.
- **Sync to Cloud:** choose which providers, Git/SSH credentials, MCP servers, skills and repos to share with the cloud host; the host clones repos on demand.
- A unified, searchable Org/Repo and Project picker on Desktop and Mobile, and one **Sessions** list for local and cloud chats.
- Per-host Claude sign-in on the cloud host, and an Auto mode indicator for cloud chats.
- **Settings:** Remote & Mobile moved under App; a new Cloud Host dashboard with a phone pairing QR code.
- **Chat:** continue work from the latest agent error; docked side chats now use the regular transcript and composer.
- **Message actions** are now compact icons (Copy, Edit, Rewind, Fork, Quote) with descriptive tooltips; Copy briefly shows a check mark.
- **Quote & ask:** selecting text in a reply opens a small popover — click the quote icon to just quote it, or type a question and press ↵ to send the quote with your question (⌥↵ adds both to your draft instead).
- Sent quotes appear as a compact block in your message, with **Jump ↑** to scroll to the quoted reply.
- **Composer context cards:** quotes and captured source, diff, terminal, console and browser excerpts now show as removable cards above the composer instead of raw text in the input. Long pastes collapse into a **Pasted text** card; hover to read it.
- **Sidebar:** the usage meter is a circular percentage ring.
- **Git:** reviewed abort, stash and recovery copies for in-progress Git operations; Git actions are summarized as compact activity lines.
- **Browser:** scoped element inspection, reviewed captures and temporary tab groups.

### Fixed

- Right-to-left prose and lists align automatically.
- Single MCP connectors reconnect correctly, and sent user messages render as Markdown.
- LiteLLM model limits and capabilities load accurately.
- Task apply-back handles executable modes, UTF-8 paths and bounded binary files.
- Session identity is consistent, and development builds no longer steal notification clicks from the installed app.
- Cloud: chunked HTTP responses, clone failures, GitHub key detection and Keychain `gh` tokens are handled correctly.

### Security

- Updated dependencies: `cookie` 0.18.3 and `url` 2.5.9 (RUSTSEC-2026-0594, RUSTSEC-2026-0592), and a mobile `minimatch` advisory (GHSA-g3hv-75wg-m249). The Linux file dialog no longer uses the unmaintained `zbus` crates.
- Upgraded `sha2` 0.11, `dirs` 7, `base64` 0.23 and `rfd` 0.17, the daemon's TypeScript 7, and the CI GitHub Actions.

### Development

- Pull requests run only the affected test suites and skip packaging; the Rust suite runs about 3x faster on CI.
- CI builds the app bundle only for release tags and manual runs, and pull requests reuse `master` caches.
- `master` is protected (pull request, review and a required **CI result** check); Dependabot and a contributing guide were added.

## [1.0.1] - 2026-11-24

### Fixed

- **Android:** the app no longer crashes on Android 15 and newer when starting its background connection service (invalid notification icon and missing notification permission).
- **Android:** notification channel setup no longer crashes in Expo Go.
- The desktop app and Android app are published together again: macOS and Android files are now attached to the same release, which is only published once both are ready.

### Pui Mobile (Android)

- New official Pui app icon, including Android adaptive and themed (monochrome) icons.
- **Notifications:** live alerts over Tailscale when Pi finishes replying, asks you a question, or waits for a tool approval. A background keep-alive keeps the connection open, and approvals can be answered with **Approve** / **Deny** right from the lock-screen notification. A Notifications section in settings shows the permission state and can send a test notification.
- **Chats:** rename, pin, fork and delete sessions from the phone; find text inside a chat; copy messages; and a scroll-to-bottom button.
- Drafts are kept when you go offline or leave a chat.
- Share text or images from other apps to Pui with the Android Share sheet.
- Subagents are shown as a tree under their parent chat.
- **Changes:** **Ask Pi** shortcuts to review the uncommitted diff or create a clean commit.
- Markdown files open in a rendered preview with a Preview/Source toggle.

### Remote & Mobile (desktop)

- The remote host supports the new mobile features: notifications, session rename/pin/fork/delete, and lock-screen approvals.

## [1.0.0] - 2026-11-23

Pui 1.0: an Android companion app, remote access to your desktop chats, safer Auto mode, and a much more stable built-in browser.

### Pui Mobile (Android)

- New Android companion app. Download the APK from this release and install it directly on your phone.
- **Pairing:** scan the QR code shown in Pui Desktop with the built-in camera viewfinder, or enter the host address manually. The Connect screen has separate tabs for both.
- **Continue any desktop chat from your phone:** live message history (newest messages load first), Markdown with inline code (including inside bold and italic text), grouped tool activity like Pui Desktop, and a verbose mode to show every tool step.
- **Composer:** single-line message box, multiple image uploads with previews in the draft and in sent messages, and a queue bar where queued messages can be promoted to **Steer**.
- Change the model, thinking level and agent permission mode from the phone; mode changes are applied by Pui Desktop exactly like a desktop change.
- Chat tabs and the verbose toggle live in a top-right ⋮ menu. Favorite chats, light/dark themes and a left-to-right layout throughout.
- Desktop and phone can work on different chats at the same time.
- Live per-provider rate limits load automatically and can be refreshed from the phone.
- The app checks GitHub Releases for new versions and offers the update. It shows a clear error when the desktop Pui is too old for the app.
- Sheets and menus no longer block scrolling on Android, and large images no longer overflow message bubbles.

### Remote & Mobile (desktop)

- New **Remote & Mobile** settings page that runs a remote host for the mobile app. By default it listens only on your Tailscale address (optionally behind `tailscale serve` for HTTPS), never on public Wi-Fi unless you choose "All interfaces". Access uses a random pairing token.
- Pairing QR code, endpoint display and connection diagnostics, with a redesigned layout.
- Chats started or continued from a phone or the web show a phone/globe badge in the sidebar.
- Saved chat history loads immediately on connect, parked chats reconnect automatically, and queued messages stay in sync between desktop and phone.

### Permissions

- When Auto blocks a risky action, you can authorize that specific action directly in the chat.
- Actions flagged by built-in safety rules go through strict classification instead of being blocked outright.
- Auto now classifies monitors and scheduled (cron) jobs instead of always asking.

### Chat and composer

- Shell code blocks in replies can be run in the project terminal with one click.
- The empty composer shows rotating placeholders with inline action hints and platform-specific shortcuts, replacing the separate permissions/shortcut footer below it.
- The composer toolbar adapts to narrow windows: model and agent-mode labels truncate, thinking effort collapses to an icon, then the pickers move to their own row so they never overlap the mic, context and send buttons. The decorative spark icon was removed.
- Model, effort and agent-mode menus stay inside the chat pane.

### Settings and updates

- **Resources:** MCP servers can be added directly (**+ Add…** opens the MCP server form when viewing MCP, and an empty MCP list offers **+ Add MCP server…**), and each server can be paused, resumed, edited or removed.
- Installing an update shows a download progress bar with a Cancel button.

### Browser

- Menus and popups that overlap the browser no longer hide the whole page; the page stays visible around them.
- Page loading is shown in the browser toolbar and no longer changes the page height.
- The page zoom setting in viewport settings no longer closes the settings popup.
- Browser pages are inset slightly so panel separators and resize handles stay visible.

### Fixed

- Fixed crashes when reopening the browser or an inline visual after closing them, when quitting Pui, and shortly after launch when the first browser page or visual opened.
- Header pills in the Changes panel no longer overlap icon controls in narrow panes.

## [0.2.2] - 2026-11-23

### Added

- Inline sign-in and API key recovery prompt above the composer when a provider rejects an OAuth subscription token or API key. Prompts let you re-login or re-submit your API key directly in the chat and automatically reload models once updated.

### Changed

- Refactored the Scheduled tasks page with a cleaner header layout, an empty-state card, and notice banners with a **Reload** action when the task store could not be read.

### Fixed

- Reconnecting an Anthropic account saved as an API key now opens the API key dialog instead of starting subscription OAuth.

## [0.2.1] - 2026-11-23

### Fixed

- The update dialog renders release notes as Markdown (headings, lists, bold and code) instead of showing raw `###` and `**` markers.
- Moving the Pi folder into Pui no longer rewrites recorded scheduled-check results. Previously this could make Scheduled tasks reject all saved jobs ("Invalid retained check evidence") after migrating.

## [0.2.0] - 2026-11-23

The biggest update since the first release: a richer chat, a new Changes panel, Pui-owned Pi settings, a stricter Auto mode, voice dictation, and Pi 1.6.0.

### Chat

- Inline photos in replies, screenshot thumbnails on activity rows (click to open full screen), and preview cards for edited HTML files and opened web pages.
- Inline visuals: the agent can draw charts, diagrams and UI mockups right in the chat with the new `pui_visual` tool. They are interactive: hover to scroll, click and type inside them.
- ⌘F finds text in the conversation.
- A floating "New messages" pill appears while you read earlier parts of a chat.
- Messages from subagents collapse into compact rows you can expand.
- Chats get a short title once, and your own renames are kept.
- Forked chats are nested under the chat they came from; you can fork from your own messages too.
- File links such as `src/main.rs:42` or `#L42`, and relative paths in inline code, open the file at that line.
- A livelier progress indicator while the agent works.

### Composer

- Voice dictation: a microphone button uses macOS speech recognition.
- A context ring shows how full the model's context is, with a warning banner before you hit usage limits.
- Single-letter shortcuts open chat menus.

### Changes and Git

- Redesigned Changes panel with an icon bar, worktrees, and read/unread state per file.
- Resolve merge conflicts by reviewing the working text and the index separately.
- Redesigned repository dialogs (clone, branches) with a folder picker and branch search.
- Busy Git reads retry automatically.

### Files, browser and Markdown

- Syntax-highlighted file viewer.
- Markdown documents get an outline popover, native Mermaid diagrams, and an expand view for wide tables.
- Browser element picker and read-only element inspection for pages in the built-in browser.
- Browser previews stay visible while the agent asks you a question.

### Inspector

- Context breakdown by category (messages, tools, skills, memory files and more), with expandable lists and the autocompact buffer.
- New Connectors and Background tasks cards, pull request actions, and previews of output files.
- Collapsed context usage shows as a ring with the percentage inside.

### Models and providers

- Bundled Pi upgraded to 1.6.0; Settings → General shows the Pi version in use.
- One accounts screen for all providers, with provider logos, model search and a compact context-size editor.
- Edit models: rename, set capabilities and limits, edit several at once, and choose a fallback model used automatically when one is rate-limited or unavailable.
- Pickers only list models from providers you have set up; hide models you don't use.
- Defaults for new chats (model and thinking effort) in Settings → Models.
- New providers appear in every chat within seconds, without restarting.
- Antigravity keeps each account's full model catalog between launches.

### Pi data

- Pui can keep its own Pi folder (providers, logins, models, defaults, skills, extensions and chats). On first launch it finds an existing `~/.pi/agent` and offers to move it in: settings are copied and chats are moved with a link left behind, so nothing is deleted. **Settings → General → Pi data** can copy settings again later.
- Pi only uses logins saved in Pui. Provider API keys in environment variables (for example a key exported as `OPENAI_API_KEY`) are no longer treated as logins; agent shell commands still receive them. An option turns environment keys back on.
- Settings no longer depend on the open chat; they read Pi's configuration directly.

### Agents, permissions and workflows

- Auto mode is stricter: more credential files are protected (including symlinks to them), disguised deletes such as `/bin/rm`, `find -delete` and `git -C … push` are blocked without asking a model, overwriting `cp`/`mv` goes to the classifier, and read-only `ls`/`git status`/`git log` work even when the classifier is down.
- The Auto classifier only uses models shown in the picker, falls back to the next cheapest model when a provider fails, and explains why a call was blocked. Choose it with the same searchable picker as the composer.
- The permission mode is chosen per chat in the composer. New chats start in the last mode you chose (Auto the first time); Bypass is never remembered as a default.
- "Don't ask again" on the Bypass confirmation, with a reset in Settings → Permissions.
- Subagents run in their parent's mode or a stricter one, never a looser one.
- Running agents keep a stable order in the sidebar.
- Workflows: recipes, a plan-revision trail, background execution that survives restarts, and fewer unnecessary workflow offers (a "no" is remembered).
- Scheduled checks keep their results so you can compare runs.

### Performance

- Agents that have been idle for ten minutes release their process and reconnect when needed.
- Lower CPU and memory use for background chats; the sidebar and session scans are cached.

### Fixed

- Chats released after being idle can change model or effort again instead of showing "Connect to Pi".
- The Background tasks card no longer widens the inspector, and durations stay clear of the scrollbar.
- Disabled primary buttons have proper contrast.
- Antigravity model lists load at startup and after a failed refresh, instead of only five built-in models.

### Docs

- The README is now a short guide with screenshots; the previous detailed text lives in `docs/reference.md`.

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
