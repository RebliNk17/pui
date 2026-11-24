# Embedded regular-chat redesign

Branch: `btw-regular-chat-redesign`, based on `95b67ea3`.

## Features

- Uses the ordinary `Transcript` renderer instead of plain-text excerpts: Markdown/code, message styling, tool/activity disclosures, images, copy/quote and normal scrolling. Embedded mode reduces gutters, not features or transcript history.
- Uses the ordinary rounded composer: model/effort/mode controls, attachments, context pills, recall/completion, queue/steer, Stop, dictation and inline requests. Expand/Close use shared icon controls.
- Side chat has separate picker/confirmation/dictation presentation state and scoped editor/control IDs; main IDs remain stable. Side permission choices do not change the new-chat default or unrelated workflow processes.
- Clipboard follows the focused editor; drops follow the pane under the pointer, never a hidden dock's stale rectangle. Asynchronous attachment results are polled for both chats.
- Keyboard ownership covers main/side requests, disabled controls, pending Bypass, native-menu formatting, popovers, background clicks and image theater. Escape stops only the scoped busy side run after question/popup handling; image Escape closes the image without stopping main.
- Side quotes drain only their own queued capture, not the global file-selection lifecycle. Main file capture preparation/binding remains intact.
- History edit/rewind/fork and resource reload require Expand. Embedded slash commands preserve these drafts rather than navigating/reconnecting while a chat is temporarily swapped in.

## Validation

Existing stable arm64 Rust, pinned CEF, Android SDK CMake/Ninja, explicit SDKROOT and `/tmp/pui-btw-clean-target`; no installations or paid model calls.

- Complete `PUI_TEST_THREADS=1 scripts/test-rust.sh`: **2,268 binary tests + 10 integration tests passed**, 2 existing ignored, zero failures. All 24 result groups completed with the unchanged runner; no omitted groups or failure retries. `/tmp/pui-btw-redesign-full-rust.log`.
- Focused side-chat coverage grew from 31 to **46 tests**: Markdown/shared controls, attachment Send/drop routing, main/side approvals and pending Bypass, default permission isolation, native IDs, image theater, Stop, completion and narrow/theme/zoom bounds. All pass in the complete suite.
- Native-menu formatting regressions: **2 passed**, including side-only Bold through the menu dispatcher. `/tmp/pui-btw-redesign-menu.log`.
- Strict offline locked **all-target Clippy passed** with `-D warnings`: `/tmp/pui-btw-redesign-clippy-final.log`.
- Changed-source rustfmt check and `git diff --check`: passed. Repository-wide fmt check also reports pre-existing formatting in `src/app/views.rs` from the base split feature; that unrelated file is not reformatted here.
- Development build passed. Native static previews inspected in dark/light: `/tmp/pui-btw-redesign-final-dark.png`, `/tmp/pui-btw-redesign-final-light.png`. These previews demonstrate presentation only, not a real model run, microphone capture or OS clipboard acceptance.
- Independent read-only review found input/capture/gallery issues; repairs and reproducing regressions are included. No definite remaining compile/deadlock issue was reported. This does not replace real paid-provider or microphone acceptance.

Earlier blocked classifier checks, the initial 300-second command timeout, visibility/fixture compile errors and focused regression failures were investigated and repaired. Final success above supersedes the earlier in-progress note; no assertion/deadline was relaxed to obtain it.

The work is isolated from concurrent edits in the original checkout. Installed application remains unchanged; no packaging, installation, deployment or app restart occurred. Push is a feature-branch publication, not integration into master.
