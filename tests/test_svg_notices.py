"""Offline packaging regressions for the additive SVG dependency notices."""
import hashlib
import json
from pathlib import Path
import tomllib
import unittest

ROOT = Path(__file__).resolve().parent.parent


class SvgNoticeTests(unittest.TestCase):
    def test_locked_renderer_versions_and_exact_notice_files_are_retained(self):
        directory = ROOT / 'assets/third-party/svg'
        manifest = json.loads((directory / 'manifest.json').read_text())
        locked = {p['name']: p['version'] for p in tomllib.loads((ROOT / 'Cargo.lock').read_text())['package']}
        packages = manifest['packages']
        self.assertEqual(len(packages), 18)
        self.assertEqual(len({p['name'] for p in packages}), 18)
        self.assertEqual(locked['resvg'], '0.50.0')
        self.assertEqual(locked['svgtypes'], '0.16.2')
        expected = {'manifest.json', 'README.txt'}
        for package in packages:
            self.assertEqual(locked[package['name']], package['version'])
            self.assertTrue(package['license'])
            self.assertTrue(package['files'])
            prefix = f"{package['name']}-{package['version']}/"
            for notice in package['files']:
                relative = notice['path']
                self.assertTrue(relative.startswith(prefix))
                self.assertNotIn('..', Path(relative).parts)
                file = directory / relative
                self.assertEqual(hashlib.sha256(file.read_bytes()).hexdigest(), notice['sha256'])
                self.assertTrue(file.read_text().strip())
                expected.add(relative)
        self.assertEqual({str(p.relative_to(directory)) for p in directory.rglob('*') if p.is_file()}, expected)

    def test_bundle_copies_notices_before_signing_and_preserves_other_resources(self):
        script = (ROOT / 'scripts/bundle-macos.sh').read_text()
        copy = 'cp -R assets/third-party/svg "$resources/ThirdParty/svg"'
        self.assertIn(copy, script)
        self.assertLess(script.index(copy), script.index('python3 scripts/sign-macos.py'))
        self.assertIn('cp -R assets/pui-engineering "$resources/pui-engineering"', script)
        self.assertIn('"$resources/"', script)
