# Browser visited history and ephemeral groups

Worker-local evidence below is historical. Integrated restore/cancellation/popup fixes, actual CEF storage acceptance and the non-green repository gate are recorded in `browser-completion-validation.md`, which supersedes the remaining integration checklist here.

## Integration hooks

`BrowserView` owns all state. The parent Files/app integration must call
`bind_owner(root, session)` before rendering a chat's browser. Same root with a
different session is a different owner; it closes the previous engine tabs and
invalidates their history recorders. Same owner does not reset state.

The single browser More menu calls `history_menu(ui, key)` and
`privacy_menu(ui, key)`; neither adds another toolbar icon. History selection
calls `Engine::navigate` on the current native tab without changing its key or
context. Main-frame CEF address notifications record visited HTTP(S) URLs and
continue updating the address field through existing `take_location`.

Call `privacy_dialog(ctx, key)` before the suspended-session early return in viewport layout. Existing native
modal masking must remain in force. It uses `confirmations::Dialog`; cancel,
Escape, backdrop, disabled confirmation, modified/repeated activation, stale
page/epoch, tab, owner or context cannot approve a staged change. Profile
creation occurs only on an explicit confirmed choice. The dialog identifies
whether it closes just this tab or all tabs in this group.

Call `session_notice(ui, key)` instead of rendering content when
`session_suspended(key)`. It draws an explicit Resume action; rendering and tab
selection never resume a context. Explicit Reload also resumes. Resume opens
the original tab address, not a hidden saved document or a visited entry.
Capture/inspection handoffs must also reject suspended or confirming sessions
and require the current live page identity; a removed native tab has default
state, not an authoritative page/epoch.

## Policy and bounds

- At most 100 visited entries across this BrowserView, each URL at most 4096
  bytes. Only credential-free HTTP(S) URLs are accepted. Entries remain local,
  are not serialized, and are never automatically copied or sent. Menu labels
  omit query and fragment values. URLs can still contain private path/query
  data: this is visited history, not a secret-removal guarantee.
- History is filtered by context/group. Clearing visited history does not clear
  cookies or affect another group's history. Removed row IDs cannot resolve.
- Default tabs use distinct context identities. At most eight named groups per
  owner may explicitly share a context. Names are at most 32 bytes; controls and
  bidi formatting overrides are rejected.
- CEF context creation still uses the existing `context_settings` and
  `context_handler`: empty cache path, session-cookie persistence disabled,
  password-manager preferences disabled. No global request context, persistent
  profile, or Keychain policy is introduced.
- Switching groups closes only the selected native tab, cancels its inspection,
  and suspends it. Clearing a group closes all and only that group's native
  tabs, releases its pooled context, removes its visited history, rotates the
  context identity, and suspends affected tabs. Other groups remain intact.
- Context disposal is asynchronous CEF teardown, not a secure erase claim, disk
  deletion, or a global Chromium data clear. No automatic page reload occurs.
  Stale callbacks cannot add history after tab close, context disposal or owner
  replacement. Screenshot/capture lifecycle hooks are integrated separately.

## Focused evidence

On the worker's isolated checkout, private target
`target-browser-session-validation`, stable Rust, CLT compiler/Git, explicit
SDKROOT, the existing Android SDK 3.22.1 CMake/Ninja, Homebrew Python 3.13 and
provided CEF 156 directory (offline; no install):

- `cargo test --offline --bin pui app::webview::history -- --test-threads=1`:
  **4 passed**. Bounds/credentials, context/stale row and callback isolation,
  masked query labels, light/dark menu clear and disabled actions.
- `cargo test --offline --bin pui app::webview::privacy -- --test-threads=1`:
  **11 passed**. Context pool reuse/disposal/failure recovery, default isolation,
  confirmed sharing, max groups/name bounds, group-only clear, cancellation,
  same-root session owner change, suspended explicit resume, dialog light/dark,
  340/760 width and 1.0/1.4 zoom, disabled/modified/repeat/Escape/stale-page guards.
- `cargo test --offline --test prompt_design -- --test-threads=1`:
  **3 passed**.
- `git diff --check`: passed.

No application/CEF runtime was launched, no full suite or profiling was run,
and no commit was made by this worker. Parent integration must run complete
validation and an actual packaged CEF probe for cookie/site-storage sharing
within a named group, isolation between groups/default tabs, group restart
rotation with another group retained, back/forward address history callbacks,
and no requests until explicit Resume. The deterministic pool tests establish
identity/reuse semantics, not Chromium's actual cookie-storage behavior.
