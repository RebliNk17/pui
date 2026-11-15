"""CircleCI environment/secret boundary tests; no network or Apple tools."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parent.parent


class CircleTests(unittest.TestCase):
    def test_circle_script_parses(self):
        subprocess.run(["bash", "-n", str(ROOT / "scripts/ci/circle-macos.sh")], check=True)

    def test_maps_revision_and_tag_but_never_protection_or_secrets(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "scripts/ci").mkdir(parents=True)
            (root / "packaging/runtime").mkdir(parents=True)
            for name in ("package.json", "package-lock.json"):
                (root / "packaging/runtime" / name).write_text("{}")
            (root / "bin").mkdir()
            (root / "scripts/ci/circle-macos.sh").write_text((ROOT / "scripts/ci/circle-macos.sh").read_text())
            (root / "scripts/ci/setup-macos.sh").write_text('''
[[ "$CI_COMMIT_SHA" == expected-revision && "$CI_COMMIT_TAG" == v1.2.3 && "$CI_PIPELINE_IID" == 42 ]]
[[ "$CI_COMMIT_REF_PROTECTED" == false && "$PUI_REQUIRE_SIGNING" == false ]]
[[ "$CARGO_HOME" == "$PWD/.cargo-ci" && "$CARGO_TARGET_DIR" == "$PWD/target" ]]
for key in PUI_GITLAB_RELEASE_TOKEN CI_JOB_TOKEN PUI_SIGN_IDENTITY PUI_SIGN_KEYCHAIN PUI_NOTARY_PROFILE PUI_SIGN_P12 PUI_SIGN_P12_BASE64 PUI_SIGN_PASSWORD PUI_APPLE_ID PUI_APPLE_TEAM_ID PUI_APPLE_APP_PASSWORD; do
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
                   "CIRCLECI": "true", "CIRCLE_SHA1": "expected-revision", "CIRCLE_TAG": "v1.2.3", "CIRCLE_BUILD_NUM": "42",
                   "CI_COMMIT_REF_PROTECTED": "true", "PUI_REQUIRE_SIGNING": "true",
                   **{key: "fake-secret" for key in ("PUI_GITLAB_RELEASE_TOKEN", "CI_JOB_TOKEN", "PUI_SIGN_IDENTITY", "PUI_SIGN_KEYCHAIN", "PUI_NOTARY_PROFILE",
                                                     "PUI_SIGN_P12", "PUI_SIGN_P12_BASE64", "PUI_SIGN_PASSWORD", "PUI_APPLE_ID",
                                                     "PUI_APPLE_TEAM_ID", "PUI_APPLE_APP_PASSWORD")}}
            result = subprocess.run(["/bin/bash", str(root / "scripts/ci/circle-macos.sh")], env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            calls = log.read_text()
            lock_test = "app::automation::tests::store_lock_restart_missed_cron_and_uncertain_delivery"
            self.assertIn(f"cargo test --locked -- --skip {lock_test}", calls)
            self.assertIn(f"cargo test --locked --bin pui {lock_test} -- --exact", calls)
            self.assertEqual(sum(line.startswith("cargo test ") for line in calls.splitlines()), 2)
            self.assertIn("bash scripts/ci/build-macos.sh", calls)
            self.assertIn("npm ci --prefix", calls)
            self.assertIn("--ignore-scripts", calls)
            self.assertNotIn("publish", calls)
            self.assertNotIn("fake-secret", calls)

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
        import importlib.util
        spec = importlib.util.spec_from_file_location("cache_key", ROOT / "scripts/ci/cache-key.py")
        cache = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cache)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "scripts/ci").mkdir(parents=True)
            (root / ".circleci").mkdir()
            (root / "src").mkdir()
            (root / "vendor/egui").mkdir(parents=True)
            (root / "packaging/runtime").mkdir(parents=True)
            for name in ("Cargo.toml", "Cargo.lock", ".circleci/config.yml", "scripts/ci/setup-macos.sh",
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

    def test_config_caches_compilers_cef_downloads_and_outputs_on_failure_with_limits(self):
        import json
        parsed = subprocess.check_output(["ruby", "-rjson", "-ryaml", "-e",
            'puts JSON.generate(YAML.load_file(ARGV[0]))', str(ROOT / ".circleci/config.yml")], text=True)
        job = json.loads(parsed)["jobs"]["macos-build"]
        self.assertEqual(job["retention"]["caches"], "3d")
        saves = [step["save_cache"] for step in job["steps"] if isinstance(step, dict) and "save_cache" in step]
        self.assertEqual(len(saves), 3)
        self.assertTrue(all(step["when"] == "always" for step in saves))
        paths = {path for step in saves for path in step["paths"]}
        self.assertTrue({"target/debug", "target/release", "target/cef-runtime", "target/agent-runtime-cache", "~/.npm/_cacache"} <= paths)
        self.assertNotIn("dist", paths)
        self.assertNotIn("~/.ssh", paths)
        self.assertNotIn("~/.npmrc", paths)
        compiled = next(step for step in saves if "target/debug" in step["paths"])
        self.assertIn('.circleci-cache/day', compiled["key"])
        self.assertIn('{{ .Branch }}', compiled["key"])
        self.assertNotIn('{{ .Revision }}', compiled["key"])

    def test_publication_is_tag_only_uncached_downstream_and_context_is_not_on_builder(self):
        parsed = subprocess.check_output(["ruby", "-rjson", "-ryaml", "-e",
            'puts JSON.generate(YAML.load_file(ARGV[0]))', str(ROOT / ".circleci/config.yml")], text=True)
        config = json.loads(parsed)
        jobs = config["workflows"]["macos"]["jobs"]
        build = next(job["macos-build"] for job in jobs if "macos-build" in job)
        publish = next(job["gitlab-release"] for job in jobs if "gitlab-release" in job)
        self.assertNotIn("context", build)
        self.assertEqual(publish["context"], ["pui-gitlab-releases"])
        self.assertEqual(publish["requires"], ["macos-build"])
        self.assertEqual(publish["filters"]["branches"]["ignore"], "/.*/")
        self.assertEqual(publish["filters"]["tags"], build["filters"]["tags"])
        self.assertIn("pipeline.project.id", publish["serial-group"])
        steps = config["jobs"]["gitlab-release"]["steps"]
        self.assertFalse(any(key in step for step in steps if isinstance(step, dict)
                             for key in ("restore_cache", "save_cache")))
        self.assertIn({"attach_workspace": {"at": "."}}, steps)
        self.assertEqual(steps[-1]["run"]["command"], "python3 -I scripts/ci/publish-gitlab.py")
        workspace = next(step["when"] for step in config["jobs"]["macos-build"]["steps"]
                         if isinstance(step, dict) and "when" in step)
        self.assertEqual(workspace["condition"]["matches"]["value"], "<< pipeline.git.tag >>")
        self.assertIn({"persist_to_workspace": {"root": ".", "paths": ["dist/releases"]}}, workspace["steps"])

    def test_refuses_non_circle_environment(self):
        result = subprocess.run(["bash", str(ROOT / "scripts/ci/circle-macos.sh")], env={"PATH": "/usr/bin:/bin"}, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"requires CircleCI", result.stderr)


spec = importlib.util.spec_from_file_location("circle_publish", ROOT / "scripts/ci/publish-gitlab.py")
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "Cargo.toml").write_text('[package]\nversion="1.2.3"\n')
        (self.root / "CHANGELOG.md").write_text('## [1.2.3]\n- Bundled runtime\n')
        runtime = self.root / "dist/Pui.app/Contents/Resources/runtime"
        runtime.mkdir(parents=True)
        (runtime / "manifest.json").write_text('{"pi":"1.5.2","node":"22.23.5"}')
        directory = self.root / "dist/releases"
        directory.mkdir()
        for ext in ("zip", "dmg"):
            (directory / f"Pui-1.2.3-macos-arm64-adhoc.{ext}").write_bytes(b"fixture")
        self.commit = "a" * 40
        self.env = {"CIRCLECI": "true", "CIRCLE_PROJECT_ID": publisher.CIRCLE_PROJECT_ID,
                    "CIRCLE_TAG": "v1.2.3", "CIRCLE_SHA1": self.commit,
                    "PUI_GITLAB_RELEASE_TOKEN": "fake-release-token", "CI_COMMIT_TAG": "v1.2.3",
                    "CI_COMMIT_SHA": self.commit}
        environment = patch.dict(os.environ, self.env, clear=True)
        environment.start()
        self.addCleanup(environment.stop)
        publisher.release.prepare(self.root)
        self.calls = []
        self.api = Mock(base=publisher.API)
        self.api.request.side_effect = self.request
        init = patch.object(publisher.release, "GitLab", return_value=self.api)
        self.init = init.start()
        self.addCleanup(init.stop)
        head = patch.object(publisher.subprocess, "check_output", return_value=self.commit + "\n")
        self.head = head.start()
        self.addCleanup(head.stop)
        self.project = {"id": int(publisher.PROJECT_ID), "path_with_namespace": publisher.PROJECT_PATH,
                        "default_branch": "master"}
        self.tag = {"name": "v1.2.3", "protected": True, "commit": {"id": self.commit}}
        self.base = {"id": self.commit}
        self.branch = {"name": "master", "commit": {"id": "b" * 40}}

    def request(self, method, endpoint, data=None, *args, **kwargs):
        self.calls.append((method, endpoint, data))
        if endpoint == f"/projects/{publisher.PROJECT_ID}":
            return self.project
        if "/repository/tags/" in endpoint:
            return self.tag
        if "/repository/branches/" in endpoint:
            return self.branch
        if "/repository/merge_base?" in endpoint:
            return self.base
        return None

    def test_verified_tag_uses_project_token_and_uploads_five_assets_then_release(self):
        self.assertEqual(publisher.main(self.root), 0)
        self.init.assert_called_once_with(publisher.API, "fake-release-token", "PRIVATE-TOKEN")
        self.assertTrue(all(c[0] == "GET" for c in self.calls[:4]))
        self.assertIn("refs%5B%5D=" + self.commit, self.calls[3][1])
        self.assertTrue(self.calls[3][1].endswith("refs%5B%5D=" + "b" * 40))
        self.assertEqual(len([c for c in self.calls if c[0] == "PUT"]), 5)
        self.assertEqual(self.calls[-1][0], "POST")
        self.assertEqual(self.calls[-1][2]["tag_name"], "v1.2.3")
        self.assertEqual(len(self.calls[-1][2]["assets"]["links"]), 5)

    def test_branch_fork_wrong_project_missing_credential_and_invalid_tag_fail_before_api(self):
        for change in ({"CIRCLECI": "false"}, {"CIRCLE_PROJECT_ID": "other"}, {"CIRCLE_BRANCH": "master"},
                       {"CIRCLE_PULL_REQUEST": "https://example.invalid/mr/1"}, {"CIRCLE_TAG": "v1.2.3-beta"},
                       {"CIRCLE_TAG": "v01.2.3"}, {"PUI_GITLAB_RELEASE_TOKEN": ""}):
            with self.subTest(change=change), patch.dict(os.environ, change):
                # Empty tokens are also rejected by the real GitLab constructor;
                # this mock must preserve that property for the gate assertion.
                if change == {"PUI_GITLAB_RELEASE_TOKEN": ""}:
                    self.init.side_effect = ValueError("empty token")
                self.assertEqual(publisher.main(self.root), 1)
                self.assertFalse(self.calls)
                self.init.side_effect = None

    def test_remote_identity_tag_protection_sha_ancestry_and_checkout_fail_before_writes(self):
        for target, key, value in ((self.project, "id", 999), (self.project, "path_with_namespace", "other/project"),
                                   (self.tag, "protected", False), (self.tag, "protected", "true"),
                                   (self.tag, "name", "v1.2.4"), (self.tag, "commit", {"id": "b" * 40}),
                                   (self.base, "id", "b" * 40), (self.project, "default_branch", None),
                                   (self.branch, "name", "other"), (self.branch, "commit", {"id": "invalid"})):
            with self.subTest(key=key, value=value), patch.dict(target, {key: value}):
                self.calls.clear()
                self.assertEqual(publisher.main(self.root), 1)
                self.assertFalse(any(c[0] != "GET" for c in self.calls))
        self.calls.clear()
        self.head.return_value = "b" * 40
        self.assertEqual(publisher.main(self.root), 1)
        self.assertFalse(self.calls)

    def test_artifact_tampering_and_mismatched_provenance_never_upload(self):
        path = self.root / "dist/releases/release.json"
        manifest = json.loads(path.read_text())
        manifest["commit"] = "b" * 40
        path.write_text(json.dumps(manifest))
        self.assertEqual(publisher.main(self.root), 1)
        self.assertFalse(any(c[0] != "GET" for c in self.calls))

    def test_network_errors_do_not_print_secrets_or_response_body(self):
        import contextlib
        import io
        self.api.request.side_effect = publisher.URLError("fake-release-token sensitive body")
        output = io.StringIO()
        with contextlib.redirect_stderr(output):
            self.assertEqual(publisher.main(self.root), 1)
        self.assertNotIn("fake-release-token", output.getvalue())
        self.assertNotIn("sensitive body", output.getvalue())


if __name__ == "__main__":
    unittest.main()
