# Rendered Markdown file block capture — bounded slice

Base: `256003a8` (includes integrated master `c55c172a`). Worker scope: local file preview/context only; no commits, provider calls, app restarts, installations, profiling, Git features or full-suite runs.

## Implemented

The local Markdown file preview now offers **Capture rendered blocks…**. Explicit activation projects the actual rendered text labels in that one loaded document, including offscreen labels; it does not parse a second approximate Markdown representation. The Quiet Mac modal lets the user select complete blocks, **Review selection**, then **Add to draft**. Initial focus is Cancel. Nothing is selected automatically, sent, executed, approved, restored as an action or interpreted as instructions.

- Original displayed Unicode, blank lines and code text are retained byte-for-byte. Separate block headings keep discontinuous selections visibly discontinuous. The exact document path is body provenance; numbered lines are explicitly **within each rendered block**, not guessed Markdown source coordinates. This file-only slice has no message identity and does not claim transcript cross-message support.
- Stable widget identity deduplicates projection offers, not text equality. Repeated identical paragraphs remain separate blocks. Projection happens once per explicit activation, including layout multipass, not once per idle frame.
- Limits: loaded source snapshot ≤512 KiB; ≤256 nonempty rendered blocks; ≤256 KiB projected text; root/path/session identity each ≤8 KiB; each cached preview ≤160 Unicode scalars plus an ellipsis. Exactly one ephemeral capture exists. No worker/thread, polling, idle repaint loop or retained whole-document capture cache is introduced. Complete source/draft snapshots exist only while this explicit capture is open.
- Accepted excerpt **including document/block provenance** must fit 24 KiB. Empty/oversized projections and selections refuse; no whole-file fallback and no truncation. Existing 16-chip/256-KiB draft checks remain whole-or-reject. Failed append consumes the reviewed action once, without partial draft changes.
- Pointer and Enter/Space/native-Tab activation freeze the bounded draft before file rendering/later composer input. Previous-live enabled launch identity is bound to layer/pass/root/session/path/load ID; current UI/owner/load are revalidated before launch. A fresh preview load lease supplies a new document ID, including same-path/same-content refresh/reopen. Review and Add compare frozen source against current loaded document text and the activation-time draft against current draft. Root/session/document switch, close/hidden preview or foreign modal invalidates instead of retargeting. Same-length draft edits also reject.
- Escape/backdrop/Cancel discard, modal keyboard/text events do not reach later surfaces, and launch ownership consumes global Send/Stop chords. Existing draft-only capture policy remains usable in demo/read-only chats; it grants no permission and emits no provider command. Ordinary native single-label selection and existing source/table/diff/console/terminal capture code are not replaced.

## Changed paths

Production:
- `src/app/context_capture/file_blocks.rs` — projection, snapshot ownership, modal, selection/review/capture lifecycle.
- `src/app/context_capture.rs` — exports, pre-activation wrapper, clear/input/collection hooks.
- `src/app/files.rs` — Markdown preview wrapper and test registration only.
- `src/app/files/budget.rs` — monotonic fresh-load identity on existing preview lease; no budget changes.
- `src/markdown/capture.rs` — forward actual scoped label text to explicit active projection.
- `src/app.rs` — **one line**, `self.prepare_file_block_capture(ctx)`, immediately before existing file-owner binding/files.show.

Tests:
- `src/app/context_capture/file_blocks/tests.rs`
- `src/app/context_capture/file_blocks/activation_tests.rs`
- `src/app/files/block_capture_tests.rs`
- `tests/prompt_design.rs` — extend existing shared-controls guard to the new modal; no weakening.

## Focused verification

Final runs (stable arm64 Rust, offline/locked, two build jobs, dedicated `/tmp/pui-file-blocks-agent218-target`, serial tests):

| Check | Result | Evidence |
| --- | --- | --- |
| `cargo test --offline --locked --bin pui app::context_capture:: -- --test-threads=1` | **68 passed**, 0 failed | `/tmp/pui-file-blocks-agent218-context-final.log` |
| `cargo test --offline --locked --bin pui block_capture_tests:: -- --test-threads=1` | **1 passed**, 0 failed | `/tmp/pui-file-blocks-agent218-viewer-final.log` |
| `cargo test --offline --locked --test prompt_design -- --test-threads=1` | **3 passed**, 0 failed | `/tmp/pui-file-blocks-agent218-prompts-final.log` |
| `cargo fmt --all -- --check` | Pass | `/tmp/pui-file-blocks-agent218-fmt-final.log` |
| `git diff --check` | Pass | Final working-tree check |

Tests exercise real modal activation→discontinuous selection→review→capture, exactly-once/no-send behavior with a command recorder and live permission record, Unicode/blank-line fidelity, empty and oversized rejection/recovery, count/text projection bounds, 90 offscreen identical paragraphs, actual egui layout multipass, duplicate widget offers, root/session/path/load/source/draft invalidation before and after review, pointer/Enter/Space/native Tab activation, later same-pass draft edits, modified-chord rejection, demo/read-only draft-only behavior, 16-chip/256-KiB refusal, hidden/foreign-modal/Cancel/Escape/backdrop cancellation, light/dark 390px viewport with 0.8/1.4 zoom, and settled repaint delay (no idle polling). Real file-viewer integration also checks same-path/same-text refresh, current loaded-document text replacement and close.

### Build setup and failed attempts

Initial cache clone from the concurrently changing integration target reported one vanished archive. No original target was changed. A first Cargo attempt failed on inherited `CARGO_HTTP_PROXY_CAINFO` without `http.proxy`; unset that variable for offline tests. The next attempt required explicit `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk` to resolve `iconv`. Cloned CEF CMake output embedded the original target path: moved **only this worker's copied** build directory to `build-cloned-unused`, then rebuilt it in the dedicated target. The fresh dependency build exceeded one 600s tool deadline; subsequent bounded invocation resumed normally. No other chat's builds were killed or modified.

Used supplied CEF distribution, recovered Ninja/CMake paths, actual CLT Git and explicit Homebrew Python 3.13. No dependency installation or network fetch occurred. One new negative test initially exposed modified Enter being accepted when global modifiers differed from event modifiers; production now inspects exact event modifiers and consumes owned Send/Stop chords. Final focused runs above are green; assertions/deadlines/scheduling were not relaxed.

## Parent integrated validation

Reviewed with all four parallel slices on base `256003a8` in `parallel-features-integration`. Complete combined validation: **2,032 Rust binary + 10 integration tests passed**,2ignored/zero failures; **114 JS passed/1skipped**, **41 Python passed**, strict all-target Clippy, formatting, shell syntax and diff checks passed. Logs `/tmp/pui-parallel-{rust,js,python,clippy,fmt}.log`. Two test-only Clippy style fixes and automation-view formatting were made during integration; no assertions/deadlines/scheduling weakened. Evidence applies to the complete combined source, not a separate full-suite run per intermediate feature commit. See `parallel-features-integration-validation.md` for integration identity and remaining acceptance gates.

## Limitations / remaining roadmap

This is one **file-only explicit complete-block** slice, not arbitrary multi-widget substring selection. The existing unsafe cross-paragraph native-drag quote remains disabled. Transcript multi-block/message provenance, PDF extraction/annotation, Mermaid rendering and richer media preview are still remaining. Tables are represented by their actual rendered cell labels, not reconstructed rows or Markdown source. Empty rendered labels are omitted. Source line numbers are intentionally unavailable.

The snapshot is the **currently loaded preview document**, not a disk watcher: external disk edits are not read automatically by this feature. Existing explicit refresh loads a fresh document identity and invalidates an open capture even if text is unchanged; replacement of current loaded text also rejects Review/Add. No automatic file reads or network activity were added.

The modal has frozen review and cancellation, not an edit/back stage; to change a reviewed selection, cancel and start again. Metadata is ephemeral and not restored. Long identity text is bounded, with only a short path preview shown in the modal; the exact bounded path remains in reviewed/captured body.

Parent must still run integrated full `scripts/test-rust.sh`, JS/Python/strict Clippy and integration review. These focused runs do not claim full-suite/application/manual visual validation or completion of the entire files/context area.
