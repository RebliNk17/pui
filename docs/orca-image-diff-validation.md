# Before/after image diff in History

## Behavior and limits

When a commit's changed file path is an image (PNG/JPEG/GIF/WebP), the History patch view offers an explicit **Image diff** button. It extracts the parent (before) and commit (after) blobs via `git cat-file blob <tree>:<path>` (bounded, 16 MiB, literal pathspec, no fetch/replace-objects), decodes both with the existing pixel-budgeted `image_preview::decode` (8192 px sides, 64 MiB decode limit), and shows them side by side (before ← → after) in a horizontal scroll. Root commits have no before (empty). No working-tree/index/HEAD mutation; read-only.

The image preview reuse is bounded and safe: no external decoders/helpers, no GPU texture leaks beyond the two cached handles, controls/controls-release and cancellation/owner guards follow the existing pattern. Blob and decode errors are shown honestly; oversized/non-decodable images are refused. Decode uses `image_preview::decode` with internal limits (no shared budget, since these are Git blobs rather than docked file-tab leases).

## Evidence

- Workspace tests: extracts differing before/after blobs; root commit empty-before; refuses bad path/identity/non-image. 2 image tests.
- App UI tests: image button renders enabled for an image path; hidden for a non-image path.
- History suite 99 passed. Full suite: **1,952 Rust + integration passed**, 2 ignored, zero failures. JS/Python pass, strict Clippy, rustfmt, diff.
- Logs `/tmp/pui-image-{focused,ui,history,clippy,full,js,python}.log`.

Committed locally; no push/install.
