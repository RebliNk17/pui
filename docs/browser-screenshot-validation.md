# Browser screenshot subsystem — bounded worker validation

Worker-local evidence below is historical. Parent integration, current APIs, actual CEF acceptance and the non-green repository gate are recorded in `browser-completion-validation.md`; that document supersedes this integration checklist.

## Implemented scope

- Explicit disclosure → current-page viewport capture → frozen editing → reviewed PNG → explicit draft-only result. No automatic network/send/save/export action; no OS/window screenshot API.
- Raw CDP `Page.getLayoutMetrics` then `Page.captureScreenshot` (`captureBeyondViewport:false`) through a dedicated asynchronous CEF observer. Browser identifier, unique page incarnation/navigation epoch, request ID, single-flight result, cancellation and ten-second host timeout checks reject stale results.
- Crop, rectangle and arrow tools, 32-annotation bound, last-32-edit Undo and Reset. Crop always targets captured pixels. PNG encoding/decoding and review rendering run on worker threads; only texture upload happens on the UI thread.
- Source viewport at most 16 million CSS pixels/16,384 per side; output at most one million pixels, 5 MiB PNG and 7 MiB CDP response. Capture scale conservatively allows up to 3× device scale; larger outputs are rejected from their PNG header. One 48 MiB lease from the existing shared 64 MiB preview pool covers encoded input, editing/review CPU/GPU overlap and attachment output.
- Shared Quiet Mac modal/heading/button/footer, safe Cancel focus, foreign-modal ownership, Escape/backdrop cancellation, modified-activation refusal, keyboard/text/paste isolation, and disabled Add when the parent draft binding is invalid/full. No persistent export.

## Parent integration contract

`BrowserView::begin_screenshot(ctx, key, draft_token)` opens disclosure for the active loaded page. `screenshot_ui(ctx, key, draft_token, can_add)` must render every visible-browser frame, including while its modal hides native Chromium; it emits one `DraftImage` only after reviewed Add. BrowserView::focused() includes screenshot utility keyboard/modal ownership; `cancel_screenshot()` must run on owner/root/session changes or leaving the browser. Ordinary own-modal native hiding must not cancel capture.

`DraftImage.page` is the page **key**, not a navigation URL; `epoch` uniquely identifies that page incarnation/navigation. Parent owns consistent page-location metadata, immutable activation binding, root/session/draft/image-revision validation, attachment-count checks, and draft append without sending. These handoff/toolbar hooks intentionally remain outside this worker's scope.

## Executed checks

Private target: `/tmp/pui-browser-screenshot-agent252-target`. Source was compiled from the managed worker worktree; no shared-target seeding. Stable Rust, Command Line Tools Git/compiler + SDKROOT, Android SDK CMake/Ninja 3.22.1, supplied CEF_PATH and `unset CARGO_HTTP_PROXY_CAINFO`.

- `cargo test -j 2 --bin pui app::webview::screenshot:: --no-fail-fast`: **5 passed, 0 failed**, 2,223 filtered. Log `/tmp/pui-browser-screenshot-agent252-test.log`.
- `cargo test -j 2 --test prompt_design`: **3 passed, 0 failed**. Log `/tmp/pui-browser-screenshot-agent252-prompts.log`.
- `rustfmt --edition 2024 src/app/webview/screenshot.rs src/app/webview/chromium/screenshot.rs`: passed (recurses into screenshot children).
- `git diff --check`: passed.

Tests exercise pointer activation through disclosure/review/Add controls, raw-CDP mock transport, real bounded PNG decode/encode workers, crop + rectangle/arrow + Undo, exactly-once draft result, malformed/base64/header limits, shared-budget refusal/release, stale/cancel/duplicate/timeout/page-incarnation handling, safe Cancel focus, modified focused Add, foreign-modal and text/paste isolation, light/dark at 260/400/900 widths with 0.8/1.0/1.6 zoom, and horizontal/vertical decision geometry. Initial narrow geometry failed genuinely; fixed by dimension-keying the anchored utility surface so an old wide Area cannot render outside a newly narrow viewport for a frame. Assertions and deadlines were not relaxed.

## Not claimed

No physical CEF/runtime smoke, temporary application launch, profiling or full suite was run. Actual CEF scale/hidden-view capture behavior requires parent-coordinated smoke. The source cap can intentionally refuse large/unsupported viewports; conservative 3× scale can reduce resolution more than necessary. Page screenshots can contain sensitive visible information; crop/annotations do not guarantee redaction. The subsystem alone is not a completed user feature until parent toolbar/handoff integration and full validation are complete. Expected dead-code warnings remain in this isolated worktree because those parent callers are not present yet.
