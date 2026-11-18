# Commit signature display in History details

> **Historical report; conflicting safety/completion claims are superseded by `git-safety-corrections.md`.** See that report for actual corrected behavior, evidence and remaining work.

## Behavior and limits

Commit details now read and show the embedded signature without invoking external gpg helpers: `git show --format=%G?%x00%GS%x00%B` parses the signature status (`%G?`: N/G/U/X/B/R/E) and signer (`%GS`) from the object itself. The detail view renders a color-coded line (good/trusted green, bad/expired/revoked/error red, unknown dim) next to the date/parents. Unsigned commits (`N`) show nothing. The list view retains `--no-show-signature`.

The status is Git's own verification label from the object's embedded signature; no cryptographic re-verification, keyring/trust DB trust, or live gpg execution is performed. Malformed/control signer text is stripped. This is display only, not an acceptance claim that a signature is trustworthy.

## Evidence

- Workspace tests: signature status/signer parse for good/bad/expired/unsigned; malformed signer controls stripped; production command includes `%G?`/`%GS` and no longer `--no-show-signature`. 21 detail tests passed.
- History UI suite: 58 passed.
- Full suite: **1,948 Rust + integration passed**, 2 ignored, zero failures. JS/Python pass, strict Clippy, rustfmt, diff.
- Logs `/tmp/pui-sig-{focused,ui,clippy,full,js,python}.log`.

Committed locally; no push/install.
