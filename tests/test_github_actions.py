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
            (root / "scripts/ci/cargo-config.toml").write_text((ROOT / "scripts/ci/cargo-config.toml").read_text())
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
            self.assertIn('opt-level = 2', (root / ".cargo-ci/config.toml").read_text())
            self.assertIn("bash scripts/ci/build-macos.sh", calls)
            self.assertIn("npm ci --prefix", calls)
            self.assertIn("--ignore-scripts", calls)
            self.assertNotIn("publish", calls)
            self.assertNotIn("fake-secret", calls)

    def test_complete_rust_runner_keeps_all_contention_groups_and_restart_test(self):
        lock_test = "app::automation::tests::store_lock_restart_missed_cron_and_uncertain_delivery"
        orphan_test = ("app::automation::delete::tests::"
                       "cross_window_orphan_delete_uses_real_transport_without_creator_and_handles_owner_exit")
        required = ("app::sync::", "app::files::delimited::projection::", "app::files::delimited::export::",
                    "app::tasks::save_conflict_tests::", "app::tasks::recovery::tests::",
                    "app::tasks::store_safety_tests::", "tasks::apply::tests::", "tasks::apply::tests::additional::",
                    "workspace::commit::tests::", "file_watch::", "sessions::watch::")
        for lanes in ("", "1"):
            with self.subTest(lanes=lanes or "parallel"), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                log = root / "commands"
                for name, body in (("cargo", 'printf "%s\\n" "$*" >> "$COMMAND_LOG"\n'), ("git", "exit 0\n")):
                    path = root / name
                    path.write_text("#!/bin/bash\n" + body)
                    path.chmod(0o755)
                env = {"PATH": f"{root}:/usr/bin:/bin", "COMMAND_LOG": str(log), "PUI_SERIAL_LANES": lanes}
                result = subprocess.run(["/bin/bash", str(ROOT / "scripts/test-rust.sh")], env=env,
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                calls = [line.split() for line in log.read_text().splitlines()]
                bulk = [c for c in calls if "--bin" not in c]
                self.assertEqual(len(bulk), 1)
                self.assertEqual(bulk[0][:5], ["test", "--offline", "--locked", "--", "--test-threads=4"])
                skips = {bulk[0][i + 1] for i, arg in enumerate(bulk[0]) if arg == "--skip"}
                serial = [c for c in calls if "--bin" in c]
                filters = [c[5] for c in serial]
                # Every serial group runs exactly once, single-threaded, and is skipped in bulk.
                self.assertEqual(len(filters), len(set(filters)))
                for call in serial:
                    self.assertEqual(call[:5], ["test", "--offline", "--locked", "--bin", "pui"])
                    self.assertEqual(call[6], "--")
                    self.assertIn("--test-threads=1", call[7:])
                    self.assertTrue(any(skip in call[5] for skip in skips), call)
                for group in required:
                    self.assertIn(group, filters)
                    self.assertIn(group, skips)
                apply = next(c for c in serial if c[5] == "tasks::apply::tests::")
                self.assertEqual(apply[8:], ["--skip", "tasks::apply::tests::additional::"])
                # OS-lock tests run last, alone and exact.
                self.assertEqual(serial[-2:], [["test", "--offline", "--locked", "--bin", "pui", test, "--",
                                                "--exact", "--test-threads=1"] for test in (lock_test, orphan_test)])
                self.assertTrue({lock_test, orphan_test} <= skips)

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
        self.assertTrue(checkout["uses"].startswith("actions/checkout@"))
        self.assertEqual(checkout["with"]["fetch-depth"], 0)
        restores = [s for s in job["steps"] if s.get("uses", "").startswith("actions/cache/restore@")]
        saves = [s for s in job["steps"] if s.get("uses", "").startswith("actions/cache/save@")]
        self.assertEqual(len(restores), 3)
        self.assertEqual(len(saves), 3)
        self.assertTrue(all("always()" in s["if"] for s in saves))
        paths = "\n".join(s["with"]["path"] for s in saves)
        for expected in ("target/debug", "target/release", "target/cef-runtime", "target/agent-runtime-cache", "~/.npm/_cacache"):
            self.assertIn(expected, paths)
        for forbidden in ("dist", "~/.ssh", "~/.npmrc"):
            self.assertNotIn(forbidden, paths)
        # Affected-suite selection gates the macOS job; partial runs never seed caches or upload.
        self.assertEqual(job["needs"], "changes")
        self.assertEqual(job["if"], "needs.changes.outputs.macos == 'true'")
        self.assertTrue(all("needs.changes.outputs.toolchain == 'true'" in s["if"] for s in saves))
        upload = next(s for s in job["steps"] if s.get("uses", "").startswith("actions/upload-artifact@"))
        self.assertEqual(upload["if"], "needs.changes.outputs.package == 'true'")
        # PRs are unfiltered so the required gate always reports; it depends on every validation job.
        self.assertIsNone(triggers["pull_request"])
        gate = config["jobs"]["ci-result"]
        self.assertEqual(gate["name"], "CI result")
        self.assertEqual(gate["if"], "always()")
        self.assertEqual(set(gate["needs"]), {"changes", "macos-build", "mobile-validate", "daemon-validate"})

    def test_affected_selects_only_changed_suites_and_fails_safe(self):
        spec = importlib.util.spec_from_file_location("affected", ROOT / "scripts/ci/affected.py")
        affected = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(affected)
        everything = {**dict.fromkeys(affected.SUITES, True), "toolchain": True, "macos": True}

        def only(*suites):
            result = {suite: suite in suites for suite in affected.SUITES}
            result["toolchain"] = result["rust"] or result["package"]
            result["macos"] = any(result[s] for s in ("rust", "node", "python", "package"))
            return result

        self.assertEqual(affected.select(["README.md", "docs/x.md", ".github/workflows/android.yml"]), only())
        self.assertEqual(affected.select(["mobile/App.tsx", "mobile/README.md"]), only("mobile"))
        self.assertEqual(affected.select(["daemon/src/a.ts"]), only("daemon"))
        self.assertFalse(affected.select(["mobile/App.tsx", "daemon/src/a.ts"])["macos"])
        self.assertEqual(affected.select(["src/app/views.rs"]), only("rust"))
        self.assertEqual(affected.select(["tests/prompt_design.rs", "vendor/egui/src/lib.rs"]), only("rust"))
        self.assertEqual(affected.select(["scripts/permissions.ts", "tests/permissions.test.mjs"]), only("node"))
        self.assertEqual(affected.select(["scripts/release.py"]), only("python", "package"))
        self.assertEqual(affected.select(["scripts/bundle-macos.sh"]), only("python", "package"))
        self.assertEqual(affected.select(["CHANGELOG.md"]), only("python"))
        for shared in ("Cargo.lock", "Cargo.toml", ".github/workflows/macos.yml", "scripts/ci/affected.py",
                       "packaging/runtime/package-lock.json"):
            self.assertEqual(affected.select(["src/main.rs", shared]), everything, shared)
        self.assertEqual(affected.select(["brand-new-dir/file"]), everything)

    def test_affected_cli_runs_everything_without_base_or_on_bad_base(self):
        script = str(ROOT / "scripts/ci/affected.py")
        for args in ([], ["0000000000000000000000000000000000000000"]):
            output = subprocess.run(["python3", script, *args], cwd=ROOT, capture_output=True, text=True, check=True).stdout
            self.assertEqual(output.split(), ["rust=true", "node=true", "python=true", "package=true", "mobile=true",
                                              "daemon=true", "toolchain=true", "macos=true"])

    def test_pull_request_runs_only_selected_suites(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "scripts/ci").mkdir(parents=True)
            (root / "tests").mkdir()
            (root / "packaging/runtime").mkdir(parents=True)
            for name in ("package.json", "package-lock.json"):
                (root / "packaging/runtime" / name).write_text("{}")
            for name in ("a.test.mjs", "streaming.test.mjs"):
                (root / "tests" / name).write_text("")
            (root / "bin").mkdir()
            (root / "scripts/ci/github-macos.sh").write_text((ROOT / "scripts/ci/github-macos.sh").read_text())
            (root / "scripts/ci/cargo-config.toml").write_text((ROOT / "scripts/ci/cargo-config.toml").read_text())
            (root / "scripts/ci/setup-macos.sh").write_text('printf "setup needs_rust=%s\\n" "$PUI_CI_NEEDS_RUST" >> "$COMMAND_LOG"\n')
            log = root / "commands"
            (root / "bin/git").write_text('#!/bin/bash\necho rev\n')
            (root / "bin/git").chmod(0o755)
            for command in ("python3", "cargo", "node", "npm", "bash"):
                path = root / "bin" / command
                path.write_text('#!/bin/bash\nprintf "%s\\n" "$(basename "$0") $*" >> "$COMMAND_LOG"\n')
                path.chmod(0o755)
            base = {"PATH": str(root / "bin") + ":/usr/bin:/bin", "COMMAND_LOG": str(log), "GITHUB_ACTIONS": "true",
                    "GITHUB_SHA": "rev", "GITHUB_RUN_NUMBER": "1", "GITHUB_REF_TYPE": "branch", "GITHUB_REF_NAME": "7/merge"}

            def run(**suites):
                log.write_text("")
                env = {**base, **{f"PUI_CI_{k.upper()}": v for k, v in suites.items()}}
                result = subprocess.run(["/bin/bash", str(root / "scripts/ci/github-macos.sh")], env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                return log.read_text()

            calls = run(rust="false", node="true", python="false", package="false")
            self.assertIn("setup needs_rust=false", calls)
            self.assertIn("node --test tests/a.test.mjs\n", calls)
            for absent in ("cargo", "unittest", "build-macos.sh", "streaming"):
                self.assertNotIn(absent, calls)
            calls = run(rust="true", node="false", python="false", package="false")
            self.assertIn("setup needs_rust=true", calls)
            self.assertIn("bash scripts/test-rust.sh", calls)
            self.assertIn("node --test tests/streaming.test.mjs\n", calls)
            self.assertNotIn("build-macos.sh", calls)
            calls = run(rust="false", node="false", python="false", package="false")
            self.assertNotIn("cargo", calls)
            self.assertNotIn("node --test", calls)
            self.assertIn("python3 scripts/release.py check", calls)
            result = subprocess.run(["/bin/bash", str(root / "scripts/ci/github-macos.sh")],
                                    env={**base, "PUI_CI_RUST": "maybe"}, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)

    def test_refuses_non_github_actions_environment(self):
        result = subprocess.run(["bash", str(ROOT / "scripts/ci/github-macos.sh")], env={"PATH": "/usr/bin:/bin"}, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"requires GitHub Actions", result.stderr)


if __name__ == "__main__":
    unittest.main()
