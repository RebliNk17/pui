"""Bundled runtime supply-chain, relocatability, and signing regressions."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import plistlib
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent.parent


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, ROOT / file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


bundle = load("runtime_bundle", "scripts/bundle-runtime-macos.py")
signing = load("runtime_signing", "scripts/sign-macos.py")
smoke = load("runtime_smoke", "scripts/smoke-runtime-macos.py")


class BundleTests(unittest.TestCase):
    def test_package_graph_is_locked_and_all_remote_entries_have_integrity(self):
        package = json.loads((ROOT / "packaging/runtime/package.json").read_text())
        lock = json.loads((ROOT / "packaging/runtime/package-lock.json").read_text())
        self.assertEqual(package["dependencies"], {"@earendil-works/pi-coding-agent": "1.6.0"})
        self.assertEqual(lock["packages"][""]["dependencies"], package["dependencies"])
        for path, entry in lock["packages"].items():
            if path:
                self.assertTrue(entry["resolved"].startswith("https://registry.npmjs.org/"))
                self.assertTrue(entry["integrity"].startswith("sha512-"))
        pins = json.loads((ROOT / "packaging/runtime/pins.json").read_text())
        self.assertEqual(pins["node"]["version"], "22.23.5")
        self.assertEqual(set(pins), {"node", "rg", "fd"})
        for pin in pins.values():
            self.assertTrue(pin["url"].startswith("https://"))
            self.assertRegex(pin["sha256"], r"^[0-9a-f]{64}$")

    def test_cached_download_is_verified_every_time_and_never_refetched_on_corruption(self):
        with tempfile.TemporaryDirectory() as temp:
            cache = Path(temp)
            archive = cache / "node.tar.gz"
            archive.write_bytes(b"verified archive")
            pin = {"url": "https://example.invalid/node.tar.gz", "sha256": hashlib.sha256(archive.read_bytes()).hexdigest()}
            with patch.object(bundle.subprocess, "run") as run:
                self.assertEqual(bundle.fetch(pin, cache), archive)
                run.assert_not_called()
                archive.write_bytes(b"modified")
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    bundle.fetch(pin, cache)
                run.assert_not_called()

    def test_unsafe_archive_paths_and_links_fail_before_extraction(self):
        for name, link in (("../escape", None), ("root/link", "/etc/passwd"), ("root/link", "../../escape")):
            with self.subTest(name=name, link=link), tempfile.TemporaryDirectory() as temp:
                path = Path(temp) / "archive.tar.gz"
                with tarfile.open(path, "w:gz") as archive:
                    entry = tarfile.TarInfo(name)
                    if link:
                        entry.type = tarfile.SYMTYPE
                        entry.linkname = link
                        archive.addfile(entry)
                    else:
                        entry.size = 1
                        archive.addfile(entry, io.BytesIO(b"x"))
                with patch.object(bundle.subprocess, "run") as run, self.assertRaises(ValueError):
                    bundle.extract(path, Path(temp) / "extracted")
                run.assert_not_called()

    def test_archive_link_chains_and_hardlinks_are_rejected(self):
        for hardlink in (False, True):
            with tempfile.TemporaryDirectory() as temp:
                path = Path(temp) / "archive.tar.gz"
                with tarfile.open(path, "w:gz") as archive:
                    for name, target in (("root/first", "second"), ("root/second", "regular")):
                        entry = tarfile.TarInfo(name)
                        entry.type = tarfile.LNKTYPE if hardlink else tarfile.SYMTYPE
                        entry.linkname = target
                        archive.addfile(entry)
                    entry = tarfile.TarInfo("root/regular")
                    entry.size = 1
                    archive.addfile(entry, io.BytesIO(b"x"))
                with patch.object(bundle.subprocess, "run") as run, self.assertRaises(ValueError):
                    bundle.extract(path, Path(temp) / "out")
                run.assert_not_called()

    def test_license_inventory_fails_closed_and_uses_reviewed_supplements(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            module = root / "node_modules/example"
            module.mkdir(parents=True)
            (module / "package.json").write_text(json.dumps({"name": "example", "version": "1.0", "license": "MIT"}))
            with self.assertRaisesRegex(ValueError, "Missing license"):
                bundle.inventory(root / "node_modules", root / "inventory.json")
            (module / "LICENSE").write_text("fixture notice")
            values = bundle.inventory(root / "node_modules", root / "inventory.json")
            self.assertEqual(values[0]["notices"], ["example/LICENSE"])
            (module / "package.json").write_text(json.dumps({"name": "@earendil-works/pi-coding-agent", "version": "1.5.2", "license": "MIT"}))
            values = bundle.inventory(root / "node_modules", root / "inventory.json")
            self.assertEqual(values[0]["supplemental_notices"], ["Pi-MIT.txt"])

    def test_launcher_preserves_cwd_arguments_and_does_not_need_global_node(self):
        with tempfile.TemporaryDirectory(prefix="pui runtime ") as temp:
            root = Path(temp)
            (root / "bin").mkdir()
            (root / "node/bin").mkdir(parents=True)
            launcher = root / "bin/pi"
            launcher.write_text((ROOT / "packaging/runtime/pi").read_text())
            launcher.chmod(0o755)
            node = root / "node/bin/node"
            node.write_text('#!/bin/sh\nprintf "%s\\n" "$PWD" "$PI_SKIP_VERSION_CHECK" "$JITI_FS_CACHE" "$@"\n')
            node.chmod(0o755)
            cwd = root / "workspace space"
            cwd.mkdir()
            result = subprocess.run([str(launcher), "--session", "path with spaces", "literal;not-shell"],
                                    cwd=cwd, env={"PATH": "/usr/bin:/bin"}, capture_output=True, text=True, check=True)
            lines = result.stdout.splitlines()
            self.assertEqual(Path(lines[0]).resolve(), cwd.resolve())
            self.assertEqual(lines[1:3], ["1", "false"])
            self.assertEqual(lines[-3:], ["--session", "path with spaces", "literal;not-shell"])
            self.assertTrue(lines[3].endswith("pi-coding-agent/dist/bundle/cli.js"))
            result = subprocess.run([str(launcher), "update"], env={"PATH": "/usr/bin:/bin"}, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b"Update Pui", result.stderr)
            for args in (("--extensions",), ("--models",), ("npm:fixture",), ("--extension", "npm:fixture"),
                         ("plugins/example",), ("git@github.com:owner/repo",), ("--extension", "self")):
                result = subprocess.run([str(launcher), "update", *args], env={"PATH": "/usr/bin:/bin"}, capture_output=True)
                self.assertEqual(result.returncode, 0, result.stderr)
            for args in (("--self",), ("--all",), ("--extensions", "--self"), ("pi",)):
                result = subprocess.run([str(launcher), "update", *args], env={"PATH": "/usr/bin:/bin"}, capture_output=True)
                self.assertNotEqual(result.returncode, 0)

    def test_smoke_requires_packaged_executable_and_every_adapter(self):
        required = ["Contents/MacOS/pui", *("Contents/Resources/" + name for name in smoke.ADAPTER_FILES)]
        for missing in required:
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as temp:
                app = Path(temp) / "Pui.app"
                for name in required:
                    if name != missing:
                        path = app / name
                        path.parent.mkdir(parents=True, exist_ok=True)
                        path.write_text("fixture")
                with patch.object(smoke.shutil, "copytree") as copy, self.assertRaisesRegex(ValueError, "Packaged Pui file is missing"):
                    smoke.smoke(app)
                copy.assert_not_called()

    def test_signer_signs_macho_addons_search_and_node_but_not_wasm(self):
        with tempfile.TemporaryDirectory() as temp:
            app = Path(temp).resolve() / "Pui.app"
            runtime = app / "Contents/Resources/runtime"
            files = [runtime / name for name in ("bin/rg", "bin/fd", "node/bin/node", "pi/native.node")]
            for path in files:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(bytes.fromhex("cffaedfe") + b"fake")
            wasm = runtime / "pi/quickjs.so"
            wasm.write_bytes(b"\0asm")
            frameworks = app / "Contents/Frameworks"
            frameworks.mkdir()
            for suffix in ("", " (GPU)", " (Renderer)", " (Plugin)", " (Alerts)"):
                (frameworks / f"Pui Browser Helper{suffix}.app").mkdir()
            with patch.dict(os.environ, {"PUI_SIGN_IDENTITY": "-"}), patch.object(signing.subprocess, "run") as run:
                signing.sign(app)
            commands = [call.args[0] for call in run.call_args_list]
            targets = [command[-1] for command in commands]
            self.assertTrue(all(str(path) in targets for path in files))
            self.assertNotIn(str(wasm), targets)
            node_command = next(command for command in commands if command[-1] == str(runtime / "node/bin/node"))
            self.assertIn("--entitlements", node_command)
            self.assertTrue(node_command[node_command.index("--entitlements") + 1].endswith("node-entitlements.plist"))
            with (ROOT / "packaging/node-entitlements.plist").open("rb") as source:
                grants = plistlib.load(source)
            self.assertTrue(grants["com.apple.security.cs.allow-jit"])
            self.assertNotIn("com.apple.security.get-task-allow", grants)


if __name__ == "__main__":
    unittest.main()
