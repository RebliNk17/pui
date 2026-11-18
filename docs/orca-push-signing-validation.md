# Reviewed opt-in Push signing

## Behavior and limits

Push confirmation adds **Sign this push (GPG push certificate; no unsigned fallback)**. Enabling it performs a signer-availability check (`git config --includes --get-regexp '^(gpg\.|user\.signingkey$|push\.gpgSign$)'`) under a separate signing timeout (default 30s, 1–300), then runs `git push --signed=yes` with `push.gpgSign=yes`. Signing failure stops the push with no unsigned fallback and no remote update. Disabled mode keeps `push.gpgSign=false` and no `--signed`. Signing combines with existing hook/diagnostics options; timeout, hooks, diagnostics, no-force/tags behavior, conservative SSH, cancellation and uncertain-completion handling are unchanged.

Hosted remote verification of an actually-signed certificate, hardware keys, pinentry and provider authentication are not claimed — these need real credentials/devices. Existing credential helpers/config remain trusted.

## Evidence

- Backend test: signed push without a working signer fails and does not publish the ref; the same ref pushes `Updated` unsigned, proving signing is the differentiator; out-of-range signing timeouts refuse. 13 push backend tests passed.
- UI test: signed push confirmation renders in light/dark with hooks/diagnostics at 0.8/1.4 zoom, Escape cancels, no network worker starts. 21 push UI tests passed.
- Full suite: **1,935 binary + 10 integration passed**, 2 ignored, zero failures. JS 98 passed/1 skipped, Python 41 passed, strict Clippy, rustfmt and diff checks.
- Logs `/tmp/pui-pushsign-{backend,ui,clippy,full,js,python}.log`.

Committed locally; no push/install yet. Same full-suite environment notes as clone/line-stage apply (Homebrew python3 first, real CLT git, build env for the JS streaming test).
