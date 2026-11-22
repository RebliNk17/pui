# Read-only element inspection

The browser's Inspect icon now toggles a native, scroll-bounded inspection panel. Enter a conservative selector (`main > h1`, `#app`, `.title`) and explicitly press **Inspect**. **Close**, Escape while the inspector owns focus, or the icon disables it. Chrome DevTools remains a separate existing button. There is no click picker in this slice.

## Snapshot and safety bounds

- Current browser instance and top document only; no iframe or shadow-root traversal, arbitrary JavaScript field, attributes, pseudo-selectors, selector lists, clicks, submissions, DOM changes, page listeners, overlays, clipboard, network or automatic chat handoff.
- Fixed reader in a CEF/CDP isolated world with `grantUniveralAccess: false`. The application never evaluates page-supplied functions; Chromium's isolated-world native DOM wrappers/prototypes separate page-defined JS getters/expandos. This relies on Chromium's isolation, not a separate security sandbox against a compromised renderer. Native errors become fixed messages, not page exception text.
- Only tag, user-supplied selector, direct text-node fragments, viewport-relative CSS rectangle, display, visibility, position, color and font size. No `value`, `innerText`, ancestor `textContent`, HTML or attributes are extracted. No generated stable-selector claim.
- Form controls, forms and their descendants, contenteditable/role=textbox ancestors, script/style/template/noscript and invisible ancestors are refused. Text is **direct only**, at most 64 child nodes / 512 UTF-16 units; nested text is deliberately absent. This is not a secret detector: ordinary visible page text may contain sensitive content. Data is local and untrusted, never instructions. Control characters and bidi direction overrides/isolates are displayed as visible `\\u{XXXX}` escapes in facts; ordinary RTL characters remain literal.
- ASCII selector: 256 bytes, six simple components (tags, IDs, classes, descendants and spaced child combinators); document traversal stops at 10,000 elements. Each evaluation has a 500ms Chromium execution budget, a 16KiB native response limit and strict typed/finite result validation. A single 2-second delayed CEF task expires the whole request; no worker thread or polling loop.
- Browser identity, request ID and navigation epoch gate each response and follow-up command. Address change (including same-document changes), main-frame navigation/load, hide/owner switch, close and renderer failure discard pending work/results. Cancellation never terminates unrelated page JavaScript; the fixed reader finishes within its Chromium budget and late results are ignored. No requery on navigation, restored tabs, reopen or ordinary repaint.
- One CEF observer registration per explicitly enabled tab, retained until its tab closes; it is passive after cancellation. The reused isolated world contains no stored results/listeners and expires with the document. Timeout tasks retain only small local state for at most two seconds.

## Validation

Focused Rust tests cover selector grammar, single-flight transport, stale request/epoch rejection, no-match/multiple/error/oversized/malformed results, native light/dark themes at 220/360/700 points and 0.8/1/1.6 zoom, Escape, disabled controls, modified Enter and modal keyboard isolation. Node VM tests exercise the fixed reader against deterministic native-DOM stand-ins, including forbidden property getters, bounded text/tree traversal and form/editable/hidden exclusions.

Validated in the worker's dedicated `/tmp/pui-browser-inspection-agent217-target` (2 Cargo jobs, serial test cases):

- `cargo test --offline --locked --bin pui app::webview:: --jobs 2 -- --test-threads=1`: **58 passed**, including 10 new inspection tests and existing console/capture/viewport regressions. Log: `/tmp/pui-browser-inspection-agent217-rust.log`.
- `cargo test --offline --locked --test prompt_design --jobs 2`: **3 passed**. Log: `/tmp/pui-browser-inspection-agent217-prompts.log`.
- `node --test tests/browser-inspection.test.mjs`: **4 passed**. Log: `/tmp/pui-browser-inspection-agent217-js.log`.
- `cargo fmt --check`, explicit included-test-file `rustfmt --check`, `git diff --check`: passed.

Development tests caught and fixed keyboard ownership after temporary native-view hiding; the full-app regression now preserves busy work, draft, navigation target and pending approval through command-Enter, command-Backspace and Escape in both themes. CDP method IDs also share one monotonic source with emulation to prevent response aliasing.

These are deterministic unit/UI tests, **not live CEF/site acceptance**. No installed app was restarted or live site inspected. Real Chromium fixture acceptance remains necessary before making runtime isolation/appearance claims. **Runtime acceptance blocker:** `Runtime::initialize` requires an executable-relative bundled CEF framework and sandbox helper; the worker executable is unbundled, and package creation was explicitly forbidden. The existing debug smoke script requires a bundled debug app and observes only screenshots/title/geometry, not this new transport. Old installed/tmp bundles cannot validate this code. No runtime bypass or old-binary substitute was used. Full repository validation belongs to parent integration.

## Parent integrated validation

Reviewed with all four parallel slices on base `256003a8` in `parallel-features-integration`. Complete combined validation: **2,032 Rust binary + 10 integration tests passed**,2ignored/zero failures; **114 JS passed/1skipped**, **41 Python passed**, strict all-target Clippy, formatting, shell syntax and diff checks passed. Logs `/tmp/pui-parallel-{rust,js,python,clippy,fmt}.log`. Two test-only Clippy style fixes and automation-view formatting were made during integration; no assertions/deadlines/scheduling weakened. Evidence applies to the complete combined source, not a separate full-suite run per intermediate feature commit. See `parallel-features-integration-validation.md` for integration identity and remaining acceptance gates.

## Deferred

Pointer picker/highlight lifecycle, iframe/shadow DOM, selector generation, richer descendant-safe text, draft-only capture handoff and real CEF fixture acceptance. No shared context-capture implementation changed.
