# Contributing to Pui

Thanks for helping. This guide covers the workflow; see the [README](README.md#build-from-source)
for setup and [test execution](docs/test-execution.md) for test details.

## Workflow

1. `master` is protected: every change lands through a pull request with one approving
   review and a passing **CI result** check. Direct and force pushes are blocked for everyone.
2. Branch from an up-to-date `master` and keep each PR to one feature or fix.
3. Use [Conventional Commits](https://www.conventionalcommits.org/): `feat(scope): …`,
   `fix(scope): …`, `ci: …`, `docs: …`, `test: …`, `refactor: …`, `chore(deps): …`.
4. Update `CHANGELOG.md` for user-visible changes and add docs for new behavior.

## Before opening a PR

Run what your change touches (CI does the same, selecting suites from the diff):

| You changed | Run locally |
|---|---|
| Rust (`src/`, `vendor/`, `tests/*.rs`) | `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, focused `cargo test <module>::`; `scripts/test-rust.sh` before merge |
| Pi extensions (`scripts/*.ts`, `tests/*.mjs`) | `node --test tests/<name>.test.mjs` |
| Python/CI scripts (`scripts/*.py`, `scripts/ci/`) | `python3 -m unittest discover -s tests -p 'test_*.py'` |
| Mobile (`mobile/`) | `npm run typecheck && npm test` in `mobile/` |
| Daemon (`daemon/`) | `npm run typecheck && npm test` in `daemon/` |

Pull requests run only the affected suites and skip the app bundle, so builds stay fast.
Pushes to `master` and release tags run everything and package the app.

## Code rules

- **Tests:** preserve existing assertions, production deadlines and real process checks.
  Fix flaky tests at the root (wait on a condition, not a sleep); never add retries or
  silently skip coverage. Contention-sensitive groups belong in `scripts/test-rust.sh`.
- **UI prompts:** use the Quiet Mac primitives (`confirmations::Dialog`, `prompt_ui::modal`);
  stock egui/rfd dialogs are rejected by `tests/prompt_design.rs`. See
  [docs/quiet-mac-prompts.md](docs/quiet-mac-prompts.md). Check light/dark themes, narrow
  windows and zoom.
- **Permissions:** never widen permission scopes or bypass disabled-action checks.
- **Dependencies:** Dependabot opens weekly grouped updates. `egui`/`epaint`/`eframe` are
  vendored with patches, and `cef`, `resvg` and `svgtypes` are exact pins; update those by hand.
  The bundled Pi SDK in `packaging/runtime` changes only via the release flow (see
  [bundled runtime](docs/bundled-runtime.md)).
- **Secrets:** never commit credentials, signing material or tokens. CI build jobs run without secrets.

## Reporting issues

Include your macOS version, the Pui version (from the app or `CHANGELOG.md`), steps to reproduce
and relevant logs. Report security problems privately to the maintainer rather than in a public issue.
