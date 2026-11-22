# Chat menu letter shortcuts

## Behavior

While a saved chat's overflow or right-click menu is open:

| Key | Action |
| --- | --- |
| R | Rename |
| P | Pin / unpin |
| M | Move to project |
| C | Copy session path |
| E | Export as Markdown |
| A | Archive / unarchive |
| H | Hide / restore |
| D | Request delete confirmation |

The menu shows right-aligned hints. Section assignment and transcript-view submenus are unchanged. Parent accelerators do not fire while a submenu is open. Hide remains disabled for pinned chats; delete still uses the existing confirmation path. Modified command/control/option chords and held-key repeats do not activate these actions. Shifted letters work. Text events are consumed while the menu renders so accelerators cannot leak into the focused draft or rename editor.

## Validation

- Focused `cargo test --offline --locked --bin pui app::sidebar:: -- --test-threads=4`: **99 passed**, including five new menu tests covering both entry points, exact chat ownership, all eight actions, inverse states, disabled hide, closed-menu isolation, modifiers/repeats, submenu ownership, Escape, draft preservation, and light/dark/zoomed layout.
- Complete runner `./scripts/test-rust.sh` attempted: bulk **1,820 passed**, two existing ignored; panel keyboard **4 passed**, vertical panel keyboard **3 passed**, prompt design **3 passed**. The overall command reached its 900-second tool deadline during the first serial task-watch-scope test. The remaining serial groups did not complete; this is **not a complete-suite pass**. No production deadline or test assertion was changed, and no failed test was retried.
- Scoped rustfmt for all three changed Rust files and `git diff --check`: passed.
- Repository-wide `cargo fmt --check` reported existing differences in unrelated Changes UI files; they were left untouched.
- Strict all-target Clippy attempted, but dependency compilation stalled at `ring` and reached the 1,200-second command deadline (an earlier attempt also timed out). **No Clippy pass claimed.**
- No installation, packaging, deployment or application profiling performed.

Logs: `/tmp/pui-chat-menu-focused.log`, `/tmp/pui-chat-menu-full-rust.log`, `/tmp/pui-chat-menu-fmt.log`, `/tmp/pui-chat-menu-clippy-final.log`.
