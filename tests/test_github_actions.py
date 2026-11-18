"""GitHub Actions environment/secret boundary and cache-key tests; no network or Apple tools."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class GitHubActionsTests(unittest.TestCase):
    def test_github_script_parses(self):
        subprocess.run(["bash", "-n", str(ROOT / "scripts/ci/github-macos.sh")], check=True)

    def test_maps_revision_and_tag_but_never_protection_or_secrets(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "scripts/ci").mkdir(parents=True)
            (root / "packaging/runtime").mkdir(parents=True)
            for name in ("package.json", "package-lock.json"):
                (root / "packaging/runtime" / name).write_text("{}")
            (root / "bin").mkdir()
            (root / "scripts/ci/github-macos.sh").write_text((ROOT / "scripts/ci/github-macos.sh").read_text())
            (root / "scripts/ci/setup-macos.sh").write_text('''
[[ "$CI_COMMIT_SHA" == expected-revision && "$CI_COMMIT_TAG" == v1.2.3 && "$CI_PIPELINE_IID" == 42 ]]
[[ "$CI_COMMIT_REF_PROTECTED" == false && "$PUI_REQUIRE_SIGNING" == false ]]
[[ "$CARGO_HOME" == "$PWD/.cargo-ci" && "$CARGO_TARGET_DIR" == "$PWD/target" ]]
[[ "$PUI_GITHUB_REPOSITORY" == RebliNk17/pui ]]
for key in PUI_GITLAB_RELEASE_TOKEN GITHUB_TOKEN GH_TOKEN CI_JOB_TOKEN PUI_SIGN_IDENTITY PUI_SIGN_KEYCHAIN PUI_NOTARY_PROFILE PUI_SIGN_P12 PUI_SIGN_P12_BASE64 PUI_SIGN_PASSWORD PUI_APPLE_ID PUI_APPLE_TEAM_ID PUI_APPLE_APP_PASSWORD; do
  [[ -z "${!key:-}" ]] || exit 91
done
''')
            log = root / "commands"
            git = root / "bin/git"
            git.write_text('#!/bin/bash\necho expected-revision\n')
            git.chmod(0o755)
            for command in ("python3", "cargo", "node", "npm", "bash"):
                path = root / "bin" / command
                path.write_text('#!/bin/bash\nprintf "%s\\n" "$(basename "$0") $*" >> "$COMMAND_LOG"\n')
                path.chmod(0o755)
            env = {"PATH": str(root / "bin") + ":/usr/bin:/bin", "COMMAND_LOG": str(log),
                   "GITHUB_ACTIONS": "true", "GITHUB_SHA": "expected-revision",
                   "GITHUB_REF_TYPE": "tag", "GITHUB_REF_NAME": "v1.2.3", "GITHUB_RUN_NUMBER": "42",
                   "GITHUB_REPOSITORY": "RebliNk17/pui",
                   "CI_COMMIT_REF_PROTECTED": "true", "PUI_REQUIRE_SIGNING": "true",
                   **{key: "fake-secret" for key in ("PUI_GITLAB_RELEASE_TOKEN", "GITHUB_TOKEN", "GH_TOKEN", "CI_JOB_TOKEN",
                                                     "PUI_SIGN_IDENTITY", "PUI_SIGN_KEYCHAIN", "PUI_NOTARY_PROFILE",
                                                     "PUI_SIGN_P12", "PUI_SIGN_P12_BASE64", "PUI_SIGN_PASSWORD", "PUI_APPLE_ID",
                                                     "PUI_APPLE_TEAM_ID", "PUI_APPLE_APP_PASSWORD")}}
            result = subprocess.run(["/bin/bash", str(root / "scripts/ci/github-macos.sh")], env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            calls = log.read_text()
            self.assertIn("bash scripts/test-rust.sh", calls)
            self.assertIn("-p test_*.py", calls)
            self.assertIn("bash scripts/ci/build-macos.sh", calls)
            self.assertIn("npm ci --prefix", calls)
            self.assertIn("--ignore-scripts", calls)
            self.assertNotIn("publish", calls)
            self.assertNotIn("fake-secret", calls)

    def test_complete_rust_runner_keeps_all_contention_groups_and_restart_test(self):
        runner = (ROOT / "scripts/test-rust.sh").read_text()
        self.assertIn('--skip "$lock_test"', runner)
        self.assertIn('"$lock_test" -- --exact --test-threads=1', runner)
        for group in ("app::sync::", "app::files::delimited::projection::", "app::files::delimited::export::",
                      "app::tasks::save_conflict_tests::"):
            self.assertIn("--skip " + group, runner)
            self.assertIn(group + " -- --test-threads=1", runner)

    def test_setup_skips_homebrew_rustup_postinstall_and_selects_exact_toolchain(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            bin_dir = root / "bin"
            bin_dir.mkdir()
            prefix = root / "rustup"
            (prefix / "bin").mkdir(parents=True)
            toolchain = root / "exact-toolchain"
            toolchain.mkdir()
            log = root / "commands"
            commands = {
                "uname": '#!/bin/bash\nif [[ "$1" == -s ]]; then echo Darwin; else echo arm64; fi\n',
                "cmake": '#!/bin/bash\nexit 0\n',
                "ninja": '#!/bin/bash\nexit 0\n',
                "node": '#!/bin/bash\nexit 0\n',
                "python3": '#!/bin/bash\nif [[ "$1" != -c ]]; then echo /fake/pinned-cef; fi\n',
                "brew": '''#!/bin/bash
printf '%s\\n' "$*" >> "$COMMAND_LOG"
if [[ "$1" == --prefix ]]; then echo "$FAKE_RUSTUP_PREFIX";
elif [[ "$*" == 'install --skip-post-install rustup' ]]; then exit 0;
else exit 92; fi
''',
            }
            for name, content in commands.items():
                path = bin_dir / name
                path.write_text(content)
                path.chmod(0o755)
            rustup = prefix / "bin/rustup"
            rustup.write_text('''#!/bin/bash
printf '%s\\n' "rustup $*" >> "$COMMAND_LOG"
if [[ "$1" == which ]]; then echo "$FAKE_TOOLCHAIN/cargo"; fi
''')
            rustup.chmod(0o755)
            for name in ("cargo", "rustc"):
                path = toolchain / name
                path.write_text('#!/bin/bash\nexit 0\n')
                path.chmod(0o755)
            env = {"PATH": str(bin_dir) + ":/usr/bin:/bin", "HOME": str(root), "COMMAND_LOG": str(log),
                   "FAKE_RUSTUP_PREFIX": str(prefix), "FAKE_TOOLCHAIN": str(toolchain),
                   "RUST_VERSION": "1.88.0", "CARGO_TARGET_DIR": str(root / "target")}
            result = subprocess.run(["/bin/bash", "-c", 'source "$1"; [[ "$CEF_PATH" == /fake/pinned-cef ]]; [[ "$(command -v cargo)" == "$FAKE_TOOLCHAIN/cargo" ]]',
                                     "test", str(ROOT / "scripts/ci/setup-macos.sh")], env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            calls = log.read_text()
            self.assertIn("install --skip-post-install rustup", calls)
            self.assertIn("rustup toolchain install 1.88.0 --profile minimal --component rustfmt --component clippy", calls)
            self.assertNotIn("install cmake", calls)
            self.assertNotIn("install node", calls)

    def test_cache_keys_invalidate_tooling_dependencies_but_not_app_edits(self):
        spec = importlib.util.spec_from_file_location("cache_key", ROOT / "scripts/ci/cache-key.py")
        cache = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cache)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "scripts/ci").mkdir(parents=True)
            (root / ".github/workflows").mkdir(parents=True)
            (root / "src").mkdir()
            (root / "vendor/egui").mkdir(parents=True)
            (root / "packaging/runtime").mkdir(parents=True)
            for name in ("Cargo.toml", "Cargo.lock", ".github/workflows/macos.yml", "scripts/ci/setup-macos.sh",
                         "scripts/fetch-chromium-macos.py", "scripts/bundle-runtime-macos.py", "packaging/runtime/pins.json",
                         "packaging/runtime/package-lock.json", "packaging/runtime/pi", "vendor/egui/lib.rs"):
                (root / name).write_text("original")
            baseline = cache.fingerprints(root, "xcode+r1.88", "arm64", "26.3")
            (root / "src/main.rs").write_text("app edit")
            self.assertEqual(cache.fingerprints(root, "xcode+r1.88", "arm64", "26.3"), baseline)
            for name in ("Cargo.lock", "scripts/ci/setup-macos.sh", "scripts/fetch-chromium-macos.py",
                         "scripts/bundle-runtime-macos.py", "packaging/runtime/pins.json", "packaging/runtime/package-lock.json",
                         "packaging/runtime/pi", "vendor/egui/lib.rs"):
                path = root / name
                path.write_text("changed")
                self.assertNotEqual(cache.fingerprints(root, "xcode+r1.88", "arm64", "26.3")[1], baseline[1])
                path.write_text("original")
            self.assertNotEqual(cache.fingerprints(root, "new-xcode+r1.88", "arm64", "26.3"), baseline)
            self.assertNotEqual(cache.fingerprints(root, "xcode+r1.88", "arm64", "26.4"), baseline)

    def test_workflow_checks_out_full_objects_and_caches_layers_always(self):
        parsed = subprocess.check_output(["ruby", "-rjson", "-ryaml", "-e",
            'puts JSON.generate(YAML.load_file(ARGV[0]))', str(ROOT / ".github/workflows/macos.yml")], text=True)
        config = json.loads(parsed)
        triggers = config.get("on") or config.get("true")
        self.assertEqual(triggers["push"]["branches"], ["master"])
        job = config["jobs"]["macos-build"]
        checkout = job["steps"][0]
        self.assertEqual(checkout["uses"], "actions/checkout@v4")
        self.assertEqual(checkout["with"]["fetch-depth"], 0)
        restores = [s for s in job["steps"] if s.get("uses") == "actions/cache/restore@v4"]
        saves = [s for s in job["steps"] if s.get("uses") == "actions/cache/save@v4"]
        self.assertEqual(len(restores), 3)
        self.assertEqual(len(saves), 3)
        self.assertTrue(all("always()" in s["if"] for s in saves))
        paths = "\n".join(s["with"]["path"] for s in saves)
        for expected in ("target/debug", "target/release", "target/cef-runtime", "target/agent-runtime-cache", "~/.npm/_cacache"):
            self.assertIn(expected, paths)
        for forbidden in ("dist", "~/.ssh", "~/.npmrc"):
            self.assertNotIn(forbidden, paths)

    def test_refuses_non_github_actions_environment(self):
        result = subprocess.run(["bash", str(ROOT / "scripts/ci/github-macos.sh")], env={"PATH": "/usr/bin:/bin"}, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"requires GitHub Actions", result.stderr)


if __name__ == "__main__":
    unittest.main()
