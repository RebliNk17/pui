# Single-download macOS runtime

Apple Silicon `Pui.app` includes the complete pinned Pi harness and Node runtime.
Users do not need to install Pi, Node.js, npm or Homebrew just to launch Pui,
connect a configured provider, manage local resources or use Pi's basic tools.

## Contents and layout

Under `Contents/Resources/runtime/`:

- `bin/pi`: relocatable launcher, preserving workspace and arguments;
- `bin/node`, `npm`, `npx`: links to the included official Node distribution;
- `bin/fd` and `bin/rg`: pinned native Apple Silicon search tools;
- `node/`: official Node **22.23.5**, npm/npx and all npm dependencies;
- `pi/`: Pi **1.5.2** and the complete locked production npm dependency graph,
  including SDK-internal modules, worker entries, WASM and native platform addon;
- `manifest.json`: versions, official archive URLs/SHA-256 pins, npm lock hash;
- `notices/`: Node's combined license, search-tool licenses, package inventories,
  and checked-in upstream notice supplements for missing npm license files.

Chromium/CEF remains in `Contents/Frameworks` with its sandboxed helpers. The new
runtime does not change CEF's sandbox or Pui's per-tool permission policy.

## Provider setup and persistent data

**Bundling does not supply a model subscription or credentials.** Use Pi's
existing provider configuration/API-key environment variables, or log in with
the bundled Pi CLI:

```sh
"/Applications/Pui.app/Contents/Resources/runtime/bin/pi" /login
```

Settings → Providers → **Copy sign-in command** copies the actual bundled
launcher path, including when the app is renamed or moved. Provider settings
show the configured catalog; they do not directly edit credentials.

For advanced Pi/MCP/extension configuration the same bundled CLI is available.
There is no global symlink/install step. Pui is not an OS sandbox; approved tools
and trusted extensions run with the user's ordinary OS permissions.

Pi keeps its existing user data at `~/.pi/agent` (or `PI_CODING_AGENT_DIR`), including
sessions, settings, project trust and authentication. The bundler never reads or
copies the build machine's Pi auth/settings. Updating/moving Pui does not migrate,
erase or replace user data. npm package installs target Pi's user/trusted-project
directories. The default global npm prefix and caches are redirected to writable
user directories, not into the app. Jiti filesystem caches are disabled for app
children; Node's compilation cache is under the user cache directory.

Core `read`, `write`, `edit`, `bash`, `find` and `grep` need no provider request to
execute, but a real conversation requires provider access. `fd`/`rg` are included
so Pi does not need its unpinned first-use binary downloads. Custom MCP servers,
user-installed extensions, native-addon builds and project commands may still
require network access, Git, Python, Xcode tools or other third-party programs.
Pui's Git features need a working Git installation; Apple's `/usr/bin/git` can
prompt to install Command Line Tools on a clean Mac. Those are external-tool
requirements, not prerequisites to start the bundled agent.

## Resolution and external override

Packaged app: `PUI_PI_BIN` override → bundled Pi. Missing bundled runtime files
produce an error; they never silently select a different global Pi. All built-in
Node subprocesses (RPC environment, usage, Settings Resources and update checks)
use the included runtime and consistent SDK selection by default.

Development/non-app builds retain managed-Pi/PATH discovery. Explicit
`PUI_PI_BIN` opts out of bundled PATH/SDK selection; supply `PUI_PI_SDK` for a
non-managed installation so helpers use the matching `dist/index.js`. Explicit
`PUI_PI_SDK` remains honored. External overrides are advanced compatibility
choices: the user is responsible for the external runtime/version.

`Pui.app/Contents/MacOS/pui --runtime-info` prints resolved paths without starting
an agent, reading provider credentials or opening a window. It is useful to check
relocation or environment overrides. `--runtime-check` additionally imports the
SDK through Pui's actual configured Node subprocess with a 20-second bound.

## Updates, signing and integrity

The bundled Pi/Node/search tools update **with Pui**, not independently. Version
checks for standalone Pi are disabled in app children; the bundled `pi update`
entry point refuses default/self/all harness updates while allowing explicit
user-resource updates (`--extensions`, `--extension`, `--models`, package sources). Do not run npm directly against the
app runtime or point user package prefixes into it. Trusted arbitrary code can
still modify writable app files: this policy is not a filesystem sandbox.

`packaging/runtime/pins.json` holds exact HTTPS archive URLs and SHA-256 values.
`packaging/runtime/package-lock.json` holds npm versions and integrity values.
Packaging extracts freshly from verified cached archives and runs `npm ci
--omit=dev --ignore-scripts`, never copies a developer install or runs dependency
lifecycle hooks. `protobufjs` is held at compatible 7.6.6 because the newer
upstream install relies on lifecycle-generated files; 7.6.6 ships its required
files without running hooks. Unknown missing package-license notices fail
packaging. Archive hashes were checked against upstream HTTPS checksum sources;
this is not a separate GPG/signature authentication of the Node release.

Signing includes every runtime Mach-O file, including extensionless Node/fd/rg
and the arm64 `.node` addon. Other-platform native addons are removed; QuickJS
WASM `.so` assets are preserved. Node and CEF helpers receive JIT entitlements
for V8. Node alone additionally receives unsigned executable memory and disabled
library-validation entitlements for V8/WASM and user native extensions. It does
**not** receive debugger access. These extra Node grants are not added to Pui or
CEF helpers. The app remains ad-hoc unless Developer ID credentials
are configured. Real notarization and hardened-runtime extension compatibility
still require live Apple validation; ad-hoc smoke tests are not notarization.

## Verification and cache policy

```sh
python3 -m unittest discover -s tests -p 'test_runtime_bundle.py' -v
./scripts/bundle-macos.sh --debug
python3 scripts/smoke-runtime-macos.py dist/Pui.app
```

The smoke test copies the runtime to a path containing spaces, uses a fresh HOME
and system-only external PATH, sets `PI_OFFLINE=1`, and supplies no provider keys.
It requires the packaged executable and adapters, and verifies versions, npm/npx,
SDK/session creation, QuickJS WASM evaluation, native arm64 addon loading, real
bundled grep/find/bash, RPC `get_state` with Pui adapter readiness, Settings
resource discovery and Pui's actual configured Node/SDK subprocess. Settings'
embedded JS helper is also checked directly from source; this is not a complete
GUI/Settings integration test.
It hashes the payload before/after and fails on runtime writes. No model prompt
or provider request is made. CI runs it after signing and before packaging.

CircleCI caches the official archives and npm downloads; dependency keys cover
the runtime pins/lock/scripts/notices. Each app receives fresh runtime extraction
and a locked npm install so executable caches cannot silently change shipped
runtime contents. Existing compiled-cache retention/snapshot limits remain in
place. The first changed-runtime pipeline will miss its dependency cache once.
