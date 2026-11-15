"""CircleCI environment/secret boundary tests; no network or Apple tools."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class CircleTests(unittest.TestCase):
    def test_circle_script_parses(self):
        subprocess.run(["bash", "-n", str(ROOT / "scripts/ci/circle-macos.sh")], check=True)

    def test_maps_revision_and_tag_but_never_protection_or_secrets(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "scripts/ci").mkdir(parents=True)
            (root / "bin").mkdir()
            (root / "scripts/ci/circle-macos.sh").write_text((ROOT / "scripts/ci/circle-macos.sh").read_text())
            (root / "scripts/ci/setup-macos.sh").write_text('''
[[ "$CI_COMMIT_SHA" == expected-revision && "$CI_COMMIT_TAG" == v1.2.3 && "$CI_PIPELINE_IID" == 42 ]]
[[ "$CI_COMMIT_REF_PROTECTED" == false && "$PUI_REQUIRE_SIGNING" == false ]]
[[ "$CARGO_HOME" == "$PWD/.cargo-ci" && "$CARGO_TARGET_DIR" == "$PWD/target" ]]
for key in CI_JOB_TOKEN PUI_SIGN_IDENTITY PUI_SIGN_KEYCHAIN PUI_NOTARY_PROFILE PUI_SIGN_P12 PUI_SIGN_P12_BASE64 PUI_SIGN_PASSWORD PUI_APPLE_ID PUI_APPLE_TEAM_ID PUI_APPLE_APP_PASSWORD; do
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
                   **{key: "fake-secret" for key in ("CI_JOB_TOKEN", "PUI_SIGN_IDENTITY", "PUI_SIGN_KEYCHAIN", "PUI_NOTARY_PROFILE",
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
            self.assertIn("@earendil-works/pi-coding-agent@1.5.0", calls)
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

    def test_refuses_non_circle_environment(self):
        result = subprocess.run(["bash", str(ROOT / "scripts/ci/circle-macos.sh")], env={"PATH": "/usr/bin:/bin"}, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"requires CircleCI", result.stderr)


if __name__ == "__main__":
    unittest.main()
