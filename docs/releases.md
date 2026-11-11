# macOS CI and releases (GitLab)

## Pipeline

`.gitlab-ci.yml` runs on merge requests, branches, and tags, suppressing duplicate
branch pipelines when an MR is open. Native Apple Silicon jobs use GitLab-hosted
`saas-macos-medium-m1` with `macos-26-xcode-26`. Change `MACOS_IMAGE` when GitLab
retires an image. Hosted macOS runners require eligible GitLab.com access and
compute quota; they are not Linux runners with a macOS Docker image.

1. **validate:macos** checks the Cargo/changelog version, shell syntax, offline
   release tests, Rust formatting/clippy/tests, and Node adapter tests.
2. **build:macos** builds both Rust binaries, bundles the pinned/hash-checked CEF
   distribution and all five sandbox helpers, signs the app, and creates ZIP/DMG
   artifacts. Jobs run sequentially after validation; failed checks prevent releases.
3. **release:gitlab** runs only on **protected stable version tags**. It verifies
   artifact provenance/checksums, uploads assets to the GitLab Generic Package
   Registry, then creates a GitLab Release using the selected changelog section.
   Only the built-in `CI_JOB_TOKEN` is used: no personal/project access token.

Release assets:

- `Pui-X.Y.Z-macos-arm64.zip` and `.dmg` for notarized releases;
- `Pui-X.Y.Z-macos-arm64-adhoc.zip` and `.dmg` without Apple credentials;
- `SHA256SUMS`, `release.json` (version, tag, commit, architecture, signing status,
  archive hashes), and `CHANGELOG.md` (release notes).

The ZIP contains `Pui.app`; the DMG contains it plus an Applications shortcut.
Pi and Node.js remain separate prerequisites. Intel/universal builds, Linux and
Windows packaging, installing Pi, and in-app update installation are out of scope.
The existing GitHub update checker is not configured by this pipeline. GitLab
update checking and in-app changelog viewing are the next phase.

## One-time GitLab setup

In **Settings → CI/CD** and **Settings → Packages and registries**:

1. Enable GitLab-hosted macOS runners and ensure compute quota is available.
2. Protect `v*` tags; restrict creation to release maintainers. A tag must be exactly
   `vX.Y.Z`, matching the stable Cargo version. Other tags fail validation.
3. Leave protected/non-protected caches isolated (GitLab's default). Never enable
   protected variables for untrusted MR pipelines. Review changes to CI/scripts
   before creating a protected tag, including tags on non-default branches.
4. Enable the Generic Package Registry and GitLab Releases. If fine-grained job
   token permissions are enabled, allow this project's jobs to read/write packages
   and releases. No cross-project token allowlist is needed.
5. Set the maximum job artifact size high enough for **both Chromium archives**
   plus metadata (recommend **1 GB**). Project limits cannot exceed instance limits;
   if your GitLab plan/instance prevents this, adapt artifact storage before use.
6. Exclude released `pui` package versions from package cleanup policies. CI job
   artifacts expire after **14 days**; the package-backed release URLs do not.
7. Disable duplicate Generic Package filenames if available. The publisher also
   refuses to replace existing bytes/notes; a conflicting release needs a new version.

The runner installs CMake, Ninja, Node 22, Python 3.13, and rustup via Homebrew.
Rust is pinned to `1.88.0` (the project's current minimum) using `RUST_VERSION`.
CEF is fetched before Cargo to reuse the existing pinned archive/hash validation.
Only Cargo registry/git downloads are cached; no build products, keychains, or
credential files are cached or included in artifacts. CEF is fetched afresh per job.

The `.cargo-ci` cache and `target` directory are local to the CI workspace. Rustup
uses the disposable hosted runner's normal home directory. Builds are native only;
`CARGO_BUILD_TARGET` is cleared in CI and rejected by the local bundler.

## Optional Apple signing/notarization

Without Apple variables, protected tags publish **clearly labeled ad-hoc** builds.
They are not Developer ID signed or Apple notarized and may be blocked by macOS
Gatekeeper. Do not disable Gatekeeper globally. Users must assess whether they
trust such a build; use notarized releases for general distribution.

Add **all** the following in **Settings → CI/CD → Variables**. Mark every value
**protected**; mask/hide secret values where GitLab allows it. Keep environment
scope `*` (the jobs do not declare a deployment environment).

| Variable | Type | Value |
| --- | --- | --- |
| `PUI_SIGN_P12_BASE64` | **File** | Recommended: base64-encoded, password-protected Developer ID Application PKCS#12 export containing the certificate + private key. GitLab supplies a temporary file path; the job decodes it privately. |
| `PUI_SIGN_PASSWORD` | Variable, secret | PKCS#12 export password (nonempty) |
| `PUI_SIGN_IDENTITY` | Variable | Full `Developer ID Application: Name (TEAMID)` signing identity |
| `PUI_APPLE_ID` | Variable | Apple ID authorized to notarize for the team |
| `PUI_APPLE_TEAM_ID` | Variable | Apple developer team ID |
| `PUI_APPLE_APP_PASSWORD` | Variable, secret | Apple app-specific password, **not** the account password |
| `PUI_REQUIRE_SIGNING` | Variable | Optional: `true` to reject unsigned tagged releases (default `false`) |

GitLab File variables are normally entered as text, so the base64 form avoids
binary corruption. Alternatively, tooling that preserves binary bytes may set
`PUI_SIGN_P12` as a protected **File** variable containing the raw `.p12`, **instead
of** `PUI_SIGN_P12_BASE64`. Configuring both fails. Never put either value in the
repository.

Credentials are used only for protected tags, never branch/MR builds. Any partial
configuration, import failure, signing failure, rejected notarization, or timeout
fails closed: it does **not** silently downgrade to ad-hoc. Do not enable
`CI_DEBUG_TRACE` for these jobs; scripts reject it, but GitLab tracing can expose
variables before script-level defenses run.

The job imports the certificate into a randomly located temporary keychain,
restores the original keychain search list, and removes the temporary keychain on
normal exit/failure/signals. Hosted runners are disposable; forced termination can
skip shell traps. Do not reuse this setup on a shared persistent Mac without
reviewing credential/process isolation and crash cleanup. The PKCS#12 File
variable itself is managed/cleaned up by GitLab Runner, not committed or archived.
Passwords are supplied to Apple's local CLI with tracing off; isolate the runner
because privileged/local processes may observe command arguments.

Signing runs inside-out: nested CEF dylibs, CEF framework, helpers, then Pui. It
uses hardened runtime + secure timestamps; `--deep` is verification-only. Only
helpers receive `com.apple.security.cs.allow-jit` for V8. CEF's process sandbox
remains enabled; this is not App Sandbox packaging. App notarization/stapling and
Gatekeeper assessment happen before making the final ZIP. The DMG is independently
signed/notarized/stapled and verified. Signing/notarization must still be exercised
with real Apple credentials, including browser/DevTools on a clean Mac; offline
command tests do not certify Gatekeeper behavior or future CEF entitlement needs.

## Release procedure

1. Update `Cargo.toml` version and `Cargo.lock` together.
2. Move release notes from `[Unreleased]` into exactly one `## [X.Y.Z]` section in
   `CHANGELOG.md` (an optional ` - YYYY-MM-DD` date is supported). Do not put secret
   details in public release notes. Only stable versions are supported for now.
3. Run locally (Python 3.11+ for the release tooling):

   ```sh
   python3 scripts/release.py check
   python3 -m unittest discover -s tests -p 'test_release.py' -v
   cargo fmt --check
   cargo clippy --locked --all-targets -- -D warnings
   cargo test --locked
   node --test tests/*.test.mjs
   ```

4. Review/commit/push through your normal process. After the default-branch checks
   pass, an authorized maintainer creates and pushes the matching protected
   `vX.Y.Z` tag. **No tag, commit, push or upload is performed by local scripts.**
5. Inspect the tag pipeline and GitLab Release. Verify `SHA256SUMS`, download/mount
   the DMG, and launch from a clean Mac, including Browser and DevTools.

Manual local packaging (ad-hoc, no publication):

```sh
./scripts/bundle-macos.sh          # --debug is still supported for local work
bash scripts/package-macos.sh
python3 scripts/release.py prepare
```

Packaging refuses a nonempty `dist/releases` to avoid stale asset mixtures. Move
previous artifacts aside before rerunning. The bundler stages a fresh app and
retains a previous `dist/Pui.app` under `dist/Pui.previous.*/Pui.app` rather than
overlaying stale CEF resources. Retained bundles consume disk; remove them manually
when no longer needed. Local signing alone does not imply notarization; the CI
wrapper performs the complete sequence.

Retrying **release:gitlab** reuses byte-identical package files and an identical
release, so a partial upload can finish without rebuilding. Rebuilding signed
assets usually changes bytes (secure timestamps); do not overwrite a published
version. If an upload conflicts, investigate and issue a new version. Package
uploads can remain if later release creation fails. No automatic deletion/rollback
is attempted. Private GitLab projects require authorized users for downloads;
`CI_JOB_TOKEN` is never shipped in the application or release manifest. Public
update discovery for a private project requires a separate distribution/auth design.

## Validation boundaries

YAML parsing, shell syntax, offline release/signing-policy tests and local
Rust/Node checks can run without GitLab/Apple access. GitLab's **CI Lint / Pipeline
Editor** should additionally validate the configuration in the actual project,
then a branch pipeline verifies runner/image/quota/artifact limits. A real protected
tag and Apple credentials are required to validate publication and notarization.
No live publication or Apple account changes are part of local implementation.
