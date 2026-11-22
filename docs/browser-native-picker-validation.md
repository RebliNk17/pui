# Native browser element picker — implementation and validation gate

Isolated branch `browser-native-element-picker` from master `c09ee75b7a3bb69063194570be3650c98673844b`, fast-forwarded to upstream `c0423a83d4f523c6199963ac8b9080f5dd1df433` for the user's subsequent explicit push request. No installation/existing-app restart. User separately approved a disposable debug app/local Chromium test; only those owned demo processes were stopped. This document records feature-specific success **and a non-green repository gate**, not a finished validated release.

## Behavior

Existing native selector inspector adds explicit **Pick on page…** / **Cancel picker**. Chromium's native `DOM.enable`/`Overlay.enable`/`Overlay.setInspectMode(searchForNode)` provides hover highlighting and consumes selection input. No page event listeners, injected overlay elements, arbitrary JavaScript field, provider call, network request or chat capture is introduced by production picker code. Existing DevTools/selector/console retained.

One picked backend node is resolved in the current top-document isolated world, then the same bounded fixed text/style/geometry reader returns facts. It refuses foreign document/shadow root/disconnected nodes, forms/editables/hidden ancestors, special text and oversized replies. It does not generate a stable selector; label says picked element. Normal visible text can be sensitive; this is not secret detection or a compromised-renderer sandbox.

Browser/request/navigation epoch guards reject late responses; one-shot pick disables inspect overlay immediately. Cancel, hide/owner switch, navigation, renderer failure, timeout and close request overlay removal. Object group released after terminal completion/cancel. Cleanup messages are best-effort CDP submissions; renderer crash cannot provide an acknowledgement. One existing CEF observer per tab; no polling/listeners. Selector whole-request timer2s/evaluation500ms; picker whole-request timer30s (including the wait for user selection), fixed picked-reader traversal bounds; no claim of an independent hard evaluator deadline on `callFunctionOn`. One delayed task retains small browser/state handles until expiry; it never re-arms after navigation/cancel. Frames/shadow roots refuse; no click-through/viewport scale acceptance claim beyond local protocol-input fixture.

Native light/dark/narrow/zoom, disabled state, explicit pointer/Enter activation, modified chords, Escape and foreign-modal isolation covered. Automatic action on navigation/restoration remains prohibited.

## Real CEF bug found and repaired

First local runtime attempts failed even for selector facts. Debug transport evidence showed `Page.getFrameTree requested=1000010 sent=1000010`, then reentrant `Page.createIsolatedWorld requested=1000011 sent=1000012`. `ExecuteDevToolsMethod` rewrote the explicit ID, so expected-reply binding rejected it. Production inspector/cleanup now use raw `send_dev_tools_message` CDP with globally unique IDs instead; parsing/epoch bounds unchanged. Failed evidence directories `.../pui-picker-acceptance-fatotga9`, `...-z0punfnb` retained. Existing selector code had never had real CEF acceptance before this task.

## Verified feature evidence

- Private Cargo target `/tmp/pui-browser-picker-owned.8tHBco`, `CARGO_MANIFEST_DIR` checked in dep-info; `--list` confirms14 inspector tests including4new picker tests. **62 browser Rust passed**, `/tmp/pui-picker-owned-focused.log`.
- Strict all-target Clippy passed `/tmp/pui-picker-owned-clippy.log`; formatting/diff checks passed.
- JS full suite149passed/1skipped; Python41passed, `/tmp/pui-browser-picker-{js,python}.log`. Python release output is mocked. Fixed-reader JS5/5 includes picked-node foreign-document/shadow/disconnected refusal.
- Actual matching-source debug app at temporary `pui-browser-picker-runtime-a84fp8vz/Pui.app`; compiled source-binary pre-sign SHA256 `d6ef528284d66250748847892c648a2df7f1dc99cb47435156e5a1b1954fd516`. Pinned Chromium copied via existing bundle helper, ad-hoc signed, fresh HOME/config/demo, localhost only, no installed app/settings touched.
- `scripts/smoke-browser-picker-macos.py`: actual CEF selector facts, password refusal, native hover/pick→facts, Escape and same-document-navigation cancellation passed. Local page mouse/pointer/click handlers did not receive selection input. Added poisoned main-world DOM/getComputedStyle overrides; no override marker endpoint requested. Final evidence `/var/folders/_f/77x7f3v56_zcs3szhzzvn_rc0000gp/T/pui-picker-acceptance-kwvw7m7a/acceptance.json`, initial matching private build `...-7kef68gb`. Only `/` and favicon requests observed. Protocol-input acceptance is not physical-pointer/hardware/screen-reader or universal Chromium isolation proof.

## Complete repository validation is NOT green

`./scripts/test-rust.sh` attempted serially without assertion/deadline changes. Initial bulk1918passed/10failed/2ignored (`/tmp/pui-browser-picker-full.log`). A shared-target baseline comparison then exposed stale compiled baseline reuse; **remaining groups from `/tmp/pui-browser-picker-remaining-rust.log` are not accepted browser-source evidence**. Created private copy-on-write target, retained seeded CEF caches/fingerprints aside, forced pui rebuild, verified new test list/source dep-info. This is build-source identity repair, not production repair. Wrong `cargo rustc --tests` attempt rejected multiple targets without execution.

Private-target complete attempt `/tmp/pui-picker-owned-full.log`: bulk **1917passed/11failed/2ignored**,191filtered (stopped before runner serial groups). Ran every remaining serial group plus3integration suites once explicitly (`/tmp/pui-picker-owned-remaining.log`, command manifest `...-remaining-results.json`): **200passed/1failed** including10integration. Overall exercised final source **2107passed/12failed/2ignored**, not a green suite. No omitted groups/automatic retries or assertions/deadlines weakened.

Eight UI failures reproduced on untouched baseline archive `c09ee75b` at `/tmp/pui-picker-baseline.1Cijw6` (separate baseline commands/logs): chip native Tab,3SVG missing control IDs,palette/resource palette,2workflow-header. Logs `/tmp/pui-picker-baseline-*.log`. No browser code in baseline. Four private-run Git deadline failures:3index/conflict bulk and1commit-dialog fixture in serial group. Not assumed environmental or repaired; remain failures. Earlier shared-target run had different Git timeouts. Timing/load was high, but that is not causal proof.

Latest master also had formatting/Clippy issues. Mechanical corrections retained separately in working diff: redundant borrows, unused import, Copy clone/default-initializer test style, provider OAuth helper groups client tuple, context/sync formatting. No runtime feature expansion or lint suppression. Initial style edit briefly omitted test variable mutability; corrected, strict Clippy passes. Browser keyboard test fixture initially forgot new Decision fields and key release; fixed fixture, no production assertions relaxed.

## Remaining / completion gate

Do not call all validation green or push/install this branch as a validated feature. Resolve baseline UI regressions and investigate Git deadlines separately, or obtain explicit scope decision for feature-only integration. After this non-green state was disclosed, the user explicitly requested `push`. Changes are committed/published with these failures documented, not labelled full-suite validated or release-ready. Full browser roadmap also retains richer safe descendant text, stable selector generation, frames/shadow roots, cropped screenshots/annotations, draft-only handoff and privacy/history UX. Temporary app/targets/evidence retained, no cleanup authorization inferred.

## Explicit push follow-up

Fetched upstream `c0423a83` (transcript animation only, no overlapping browser changes), fast-forwarded and applied one formatting-only correction to its new progress renderer. No full-suite rerun after this upstream addition; prior full-suite failures remain the repository acceptance state. On the reconciled source: browser62/62, upstream progress5/5, prompt architecture3/3, fixed-reader JS5/5, strict all-target Clippy, formatting and diff checks passed. Logs `/tmp/pui-picker-push-{focused,upstream,prompts,js,clippy,fmt}.log`. Actual CEF evidence above was built before the unrelated upstream animation, not a new runtime test of the reconciled tip. No installation or restart authorized by the push request.
