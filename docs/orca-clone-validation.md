# Reviewed terminal-free repository clone

## Behavior and limits

Changes panel adds **Clone repository…**: URL (credential-free, `push::remote::valid_url` policy), destination folder name (255 bytes, no slashes/controls), optional start branch, and a 1–300s timeout (default 30). Worker review freezes parent/name/URL/branch; confirmation describes destination and default-or-chosen branch. Apply re-inspects, then `git clone --quiet` via the existing conservative `network_command` (SSH BatchMode, no interactive login, askpass false, hooks disabled). No force, push, tags or hooks. On timeout/cancel the destination completion is uncertain; no automatic retry and no automatic cleanup.

Parent must be an existing canonical absolute directory; destination must be absent; URL and branch validated. Existing credential helpers/config remain trusted; hosted authentication, hardware-key/pinentry and physical acceptance are not claimed. Reuses the shared Git permit and cancellation/owner/session guards; late results after owner change, hidden view or read-only state are discarded.

Evidence:
- Workspace tests: disposable local clone checks out the branch, clean status, refuses existing destination, bad URL/name/noncanonical parent, out-of-range timeouts. App test: light/dark420pt0.8/1.4zoom Escape confirmation never contacts network.
- Full suite bulk 1785 passed/2 ignored. Serial Git/commit groups pass in isolation; occasional 3s Git-deadline flakes under concurrent agent load (system load 30+) are pre-existing and unrelated to clone.
- Strict Clippy `-D warnings`, rustfmt and diff checks passed.
- Logs: `/tmp/pui-clone-{focused,all,clippy,full2,full3,commit-group,wscommit,projection}.log`.

Committed locally as `065b9c01`. No push/install; installed app unchanged. Live clone to a hosted remote was not exercised.