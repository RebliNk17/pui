"""Offline bundle/packaging identity regressions; no Cargo builds or Apple tools."""
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class BundleIdentityTests(unittest.TestCase):
    def command(self, path, source):
        path.write_text("#!/bin/bash\nset -eu\n" + source)
        path.chmod(0o755)

    def fixture(self, root):
        scripts = root / "scripts"
        scripts.mkdir()
        for name in ("bundle-macos.sh", "bundle-chromium-macos.py", "package-macos.sh"):
            shutil.copy2(ROOT / "scripts" / name, scripts / name)
        (scripts / "release.py").write_text('print("1.2.3")\n')
        for name in ("bundle-runtime-macos.py", "sign-macos.py"):
            (scripts / name).write_text("")
        # Empty adapter files are sufficient: this tests packaging, not runtime content.
        for name in ("ultracode", "permissions", "compaction", "context", "titles", "rewind",
                     "questions", "open", "browser", "visual", "turn_ux", "pr_tools", "shellenv",
                     "workflows", "antigravity", "accounts", "model-fallback"):
            (scripts / (name + ".ts")).touch()
        (scripts / "dictation.swift").touch()
        (root / "assets/pui-engineering").mkdir(parents=True)
        (root / "assets/third-party/svg").mkdir(parents=True)
        (root / "assets/icon.png").touch()
        (root / "packaging").mkdir()
        shutil.copy2(ROOT / "packaging/Info.plist", root / "packaging/Info.plist")
        for profile in ("debug", "release"):
            target = root / "target" / profile
            target.mkdir(parents=True)
            for name in ("pui", "pui-browser-helper"):
                (target / name).write_text(profile)
        cef = root / "cef"
        (cef / "Release/Chromium Embedded Framework.framework").mkdir(parents=True)
        for name in ("archive.json", "LICENSE.txt", "CREDITS.html"):
            (cef / name).write_text("{}")
        bin_dir = root / "bin"
        bin_dir.mkdir()
        self.command(bin_dir / "uname", 'if [[ "$1" == -s ]]; then echo Darwin; else echo arm64; fi\n')
        self.command(bin_dir / "cargo", '''printf '%s\n' "$*" >> "$CARGO_LOG"
if [[ "$1" == pkgid ]]; then echo 'path+file:///fake#pui@1.2.3'; fi
''')
        for name in ("cmake", "ninja", "sips", "plutil"):
            self.command(bin_dir / name, "exit 0\n")
        self.command(bin_dir / "iconutil", 'touch "${@: -1}"\n')
        (bin_dir / "python3").symlink_to(sys.executable)
        return {**os.environ, "PATH": str(bin_dir) + ":/usr/bin:/bin", "CEF_PATH": str(cef),
                "CARGO_LOG": str(root / "cargo-log"), "CARGO_BUILD_TARGET": "",
                "CARGO_TARGET_DIR": str(root / "target"), "CI_PIPELINE_IID": "42",
                "PUI_SIGN_IDENTITY": "-", "PUI_SIGNING_STATUS": "invalid-test-status"}

    def test_local_and_distribution_identities_are_independent_of_optimization(self):
        for args, profile, bundle_id, display in (
            ([], "release", "dev.pi.pui.dev", "Pui Dev"),
            (["--debug"], "debug", "dev.pi.pui.dev", "Pui Dev"),
            (["--distribution"], "release", "dev.pi.pui", "Pui"),
            (["--debug", "--distribution"], "debug", "dev.pi.pui", "Pui"),
            (["--distribution", "--debug"], "debug", "dev.pi.pui", "Pui"),
        ):
            with self.subTest(args=args), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                env = self.fixture(root)
                previous = root / "dist/Pui.app/Contents"
                previous.mkdir(parents=True)
                old_plist = plistlib.dumps({"CFBundleIdentifier": "dev.pi.pui", "marker": "untouched"})
                (previous / "Info.plist").write_bytes(old_plist)
                result = subprocess.run(["/bin/bash", str(root / "scripts/bundle-macos.sh"), *args],
                                        env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                retained = list((root / "dist").glob("Pui.previous.*/Pui.app/Contents/Info.plist"))
                self.assertEqual(len(retained), 1)
                self.assertEqual(retained[0].read_bytes(), old_plist)
                app = root / "dist/Pui.app"
                info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
                self.assertEqual(info["CFBundleIdentifier"], bundle_id)
                self.assertEqual(info["CFBundleName"], display)
                self.assertEqual(info["CFBundleDisplayName"], display)
                self.assertEqual(info["CFBundleShortVersionString"], "1.2.3")
                self.assertEqual(info["CFBundleVersion"], "42")
                self.assertEqual((app / "Contents/MacOS/pui").read_text(), profile)
                cargo = Path(env["CARGO_LOG"]).read_text()
                self.assertIn("build --bins --release --locked" if profile == "release"
                              else "build --bins --locked", cargo)
                for suffix in ("", " (GPU)", " (Renderer)", " (Plugin)", " (Alerts)"):
                    helper = app / f"Contents/Frameworks/Pui Browser Helper{suffix}.app/Contents"
                    helper_info = plistlib.loads((helper / "Info.plist").read_bytes())
                    expected = bundle_id + ".browser-helper" + suffix.replace(" (", ".").replace(")", "").lower()
                    self.assertEqual(helper_info["CFBundleIdentifier"], expected)
                    self.assertEqual((helper / "MacOS" / f"Pui Browser Helper{suffix}").read_text(), profile)

    def test_release_packaging_rejects_dev_or_unknown_identity_before_apple_tools(self):
        for bundle_id in ("dev.pi.pui.dev", "other.app", None, "dev.pi.pui"):
            with self.subTest(bundle_id=bundle_id), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                env = self.fixture(root)
                contents = root / "dist/Pui.app/Contents"
                contents.mkdir(parents=True)
                info = {} if bundle_id is None else {"CFBundleIdentifier": bundle_id}
                (contents / "Info.plist").write_bytes(plistlib.dumps(info))
                result = subprocess.run(["/bin/bash", str(root / "scripts/package-macos.sh")],
                                        env=env, capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                if bundle_id == "dev.pi.pui":
                    self.assertIn("Unknown signing status", result.stderr)
                    self.assertNotIn("distribution bundle identity", result.stderr)
                else:
                    self.assertIn("distribution bundle identity", result.stderr)
                    self.assertNotIn("Unknown signing status", result.stderr)
                self.assertFalse((root / "dist/releases").exists())

    def test_unknown_bundle_flag_fails_before_build(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            env = self.fixture(root)
            result = subprocess.run(["/bin/bash", str(root / "scripts/bundle-macos.sh"), "--unknown"],
                                    env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("[--debug] [--distribution]", result.stderr)
            self.assertFalse(Path(env["CARGO_LOG"]).exists())


if __name__ == "__main__":
    unittest.main()
