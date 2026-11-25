# Automatic updates and composer recovery

## Agreed behavior

- Automatically check: **ON**, existing six-hour background interval/manual check.
- Automatically download: **ON** for new and older preferences.
- Automatically install: **OFF**, opt-in only. Download OFF immediately forces
  install OFF and disables its shared Settings/Updates control. Re-enabling
  downloads never restores installation consent.
- Opted-in installation requires every agent actually sleeping/stopped, no live
  clients or retirement, and ten minutes without application interaction.
  Foreground is allowed: reading without interaction can trigger installation.
- Incremental downloads are assessed here, not implemented in this change.

## Implementation and safety

`updates.rs` claims automatic downloads once per version per app session, without
opening a modal. Cancel/discard/failure consume that session's automatic attempt;
manual retry works. Turning downloads off cancels only automatically started
workers. Check/download requests do not overlap and newer releases supersede
older staged offers. Tests/demo never start automatic network workers.

Shared Quiet Mac controls appear in General and Updates; the modal body scrolls
and primary actions stack at narrow/zoomed sizes. The renderer requests manual
installation; it cannot spawn an installer or close the app.

`updates/idle.rs` observes input before event consumption, plus native-menu and
remote actions, using a monotonic ten-minute deadline. It evaluates at frame end,
after agent/task/automation polling and UI actions. Unknown/unhealthy/disabled
window coordination fails closed. Other windows, approvals, queues, terminal
shells, active tasks, automation deliveries/commands, imports, dictation,
permission/workspace changes and other unsafe transitions defer installation.
Open settings, files/browser panes, forms and modals also defer unattended
restart. Remote hosting conservatively defers restart while enabled. Explicit
Install & Relaunch may close settled idle clients without waiting for sleep;
it still blocks work, requires a durable save and drains late RPC activity.

`restart.rs` retains exact text (no former 256K-character truncation), ready
image/file attachments and unsent history-draft shelves. Active/parked/blank
composers are retained without the old 128-record cap. Default startup hydrates
parked composers without launching processes; recovered side chats are ordinary
chats, not recreated dock pairs. Explicit command-line cwd/session overrides keep
existing selection semantics. Context capture bodies remain plain draft text;
no chip associations, approvals, tool operations or messages are replayed. Failed
or unfinished attachment imports block update restart rather than disappearing.

`restart/recovery.rs` captures preferences/composers, attention and checked local
review notes into a private 0600 snapshot in a 0700 directory under eframe's Pui
storage. Write, file fsync, same-directory atomic rename and directory fsync must
succeed before relaunch; the envelope has a 256 MiB safety bound, never truncates
payloads, and a failed save keeps Pui open. eframe's ordinary asynchronous,
errorless flush is **not** treated as durable acknowledgement. Startup overlays
unacknowledged recovery keys and copies them into real frame storage. A marker
read at a subsequent launch acknowledges regular persistence; conflicting
unrelated markers retain the snapshot without blindly replaying it. Repeated
preflights preserve their disk ancestry, including intermediate acknowledged
markers, rather than requiring an unpersisted marker.

Every new-build GUI holds `update_gate.rs`'s shared lock through eframe/Chromium
teardown. The Rust installer helper acknowledges initialization, waits boundedly
for exclusive ownership and parent exit, verifies the staged bundle again,
copies/verifies a candidate beside the target (cross-volume safe), and renames
with rollback. Errors after exit reopen the unchanged original when available.
It never kills peer windows. Old builds do not honor this lock; positive live
registry single-window checking is retained too. Helpers clean only their own
staging/candidate paths; unsafe global staging sweeps were removed.

## Validation

Focused tests cover preference migration/dependency and disabled controls;
auto-download suppression/dedup/cancellation; light/dark/narrow/zoom modal layout
and Escape; manual request-only semantics; large Unicode drafts, images/files,
parked/blank/side composers, >128 retention, pending navigation and placeholder
protection; failed and repeated durable saves; late active/parked editor RPC;
shared/exclusive window guards; and replacement rollback.

- Complete final `bash scripts/test-rust.sh`: **2,393 passed, 0 failed, 2 existing
  ignored**, all 24 runner groups included once. Log:
  `/tmp/pui-automatic-updates-final-rust.log`.
- `cargo clippy --offline --locked --all-targets`: completed with existing
  cloud/remote warnings, no new updater warnings.
- Targeted rustfmt for every changed Rust file and `git diff --check`: passed.
- Existing prompt-design guard: three tests passed; modal layout/input regressions
  pass in both themes and multiple widths/zoom levels.
- Independent read-only review found recovery ancestry, final RPC-drain and
  post-exit failure-reopen issues; each was repaired and regression coverage added.
  A subsequent failure-path review found that deleting a published journal after
  directory fsync failure could discard both recovery generations. The replacement
  is now retained, installation still aborts, and injected directory-sync failure
  coverage verifies the retained snapshot. Final lint log:
  `/tmp/pui-automatic-updates-clippy.log`.

No live release download, installed-app replacement/restart, signing/notarization,
release/tag publication or actual UI screenshot capture was performed. File-lock
and rollback fixtures use temporary paths; no application bundle was replaced.
A real packaged end-to-end update remains a release smoke-test requirement.

## Smaller/faster updates: feasible follow-up

Today `scripts/package-macos.sh` produces a monolithic ZIP/DMG containing the Rust
app, Chromium/CEF and pinned Node/Pi/search runtime. Re-downloading unchanged
Chromium/runtime assets is avoidable, but changing that safely requires release
production changes as well as the updater.

Recommended first iteration: **content-addressed components**, not arbitrary
binary patching. Publish a versioned manifest identifying app/helpers, CEF and
runtime component archives by SHA-256, sizes and exact versions. Reuse a local
component only after verifying its full identity/content. Download changed
components, reconstruct a fresh complete staging bundle with exact permissions,
framework symlinks and signing metadata, then perform final bundle identity and
`codesign --verify --deep --strict` checks before the same installer boundary.
Never modify files inside the running signed bundle. Keep the full ZIP as a
fallback for unknown versions, corruption, unsupported manifests or reconstruction
failure, with bounded retries and enough disk-space checks for both paths.

Final signing can change nested bytes/metadata even when source versions match;
component boundaries and manifests must be generated from final release bytes,
not guessed from package names. Authenticated manifest provenance/signing and
rollback/version policy should be designed explicitly; a downloaded checksum
alone is not independent publisher authentication. Begin with unchanged CEF and
runtime reuse, measuring real download savings before adding bsdiff-style patches.
No specific percentage or speed improvement is claimed without measurements.
