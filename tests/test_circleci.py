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
            for command in ("python3", "cargo", "node", "bash"):
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
            self.assertIn("cargo test --locked", calls)
            self.assertIn("bash scripts/ci/build-macos.sh", calls)
            self.assertNotIn("publish", calls)
            self.assertNotIn("fake-secret", calls)

    def test_refuses_non_circle_environment(self):
        result = subprocess.run(["bash", str(ROOT / "scripts/ci/circle-macos.sh")], env={"PATH": "/usr/bin:/bin"}, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"requires CircleCI", result.stderr)


if __name__ == "__main__":
    unittest.main()
