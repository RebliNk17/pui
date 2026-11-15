"""Offline release regressions. Run with Python 3.11+ unittest discovery."""
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parent.parent


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, ROOT / file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


release = load("release", "scripts/release.py")
signer = load("signer", "scripts/sign-macos.py")


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "Cargo.toml").write_text('[package]\nversion = "1.2.3"\n')
        (self.root / "CHANGELOG.md").write_text(
            '# Changelog\n## [Unreleased]\n- Future work\n## [1.2.3] - 2026-11-11\n### Added\n- Current feature\n## [1.2.2]\n- Old work\n')
        self.env = {"CI_COMMIT_TAG": "v1.2.3", "CI_COMMIT_SHA": "a" * 40,
                    "CI_COMMIT_REF_PROTECTED": "true", "CI_API_V4_URL": "https://gitlab.example/api/v4",
                    "CI_PROJECT_ID": "123", "CI_JOB_TOKEN": "fake-job-token"}
        self.environment = patch.dict(os.environ, self.env, clear=True)
        self.environment.start()
        self.addCleanup(self.environment.stop)

    def prepare(self, signing="ad-hoc"):
        directory = self.root / "dist/releases"
        directory.mkdir(parents=True, exist_ok=True)
        suffix = "-adhoc" if signing == "ad-hoc" else ""
        for ext in ("zip", "dmg"):
            (directory / f"Pui-1.2.3-macos-arm64{suffix}.{ext}").write_bytes(b"fake app")
        runtime = self.root / "dist/Pui.app/Contents/Resources/runtime"
        runtime.mkdir(parents=True, exist_ok=True)
        (runtime / "manifest.json").write_text(json.dumps({"schema": 1, "node": "22.23.5", "pi": "1.5.2", "platform": "darwin-arm64"}))
        with patch.dict(os.environ, {"PUI_SIGNING_STATUS": signing}):
            release.prepare(self.root)
        return directory

    def test_tag_and_cargo_version_must_agree(self):
        self.assertEqual(release.check(self.root, "v1.2.3"), "1.2.3")
        for tag in ("1.2.3", "v1.2.4", "v1.2.3-beta.1", "v01.2.3", "v1.2.3\n"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                release.check(self.root, tag)
        for value in ("01.2.3", "1.2.3-beta.1", "1.2.3+build.4"):
            (self.root / "Cargo.toml").write_text(f'[package]\nversion = "{value}"\n')
            with self.assertRaises(ValueError):
                release.version(self.root)

    def test_extracts_only_matching_notes_and_rejects_missing_empty_duplicate(self):
        self.assertEqual(release.notes(self.root, "1.2.3"), "### Added\n- Current feature\n")
        for text in ("## [Unreleased]\n- Future", "## [1.2.3]\n\n## [1.2.2]\nOld",
                     "## [1.2.3]\nFirst\n## [1.2.3]\nSecond"):
            (self.root / "CHANGELOG.md").write_text(text)
            with self.assertRaises(ValueError):
                release.notes(self.root, "1.2.3")

    def test_manifest_notes_and_checksums_label_signing_truthfully(self):
        directory = self.prepare()
        manifest = json.loads((directory / "release.json").read_text())
        self.assertEqual(manifest["commit"], "a" * 40)
        self.assertEqual(manifest["signing"], "ad-hoc")
        self.assertEqual(manifest["architecture"], "arm64")
        self.assertEqual(manifest["runtime"]["pi"], "1.5.2")
        self.assertEqual(manifest["runtime"]["node"], "22.23.5")
        self.assertTrue(all("-adhoc." in name for name in manifest["files"]))
        notes = (directory / "CHANGELOG.md").read_text()
        self.assertIn("not Developer ID signed or notarized", notes)
        self.assertNotIn("Old work", notes)
        self.assertNotIn("Future work", notes)
        for line in (directory / "SHA256SUMS").read_text().splitlines():
            sha, name = line.split("  ")
            self.assertEqual(release.digest(directory / name), sha)
        self.assertEqual(len((directory / "SHA256SUMS").read_text().splitlines()), 4)

    def test_notarized_manifest_has_no_adhoc_warning(self):
        directory = self.prepare("developer-id-notarized")
        self.assertNotIn("Ad-hoc build", (directory / "CHANGELOG.md").read_text())
        self.assertTrue(all("-adhoc" not in name for name in json.loads((directory / "release.json").read_text())["files"]))

    def test_publish_uses_durable_assets_job_token_and_changelog(self):
        self.prepare()
        calls = []

        def request(method, endpoint, data=None, *args, **kwargs):
            calls.append((method, endpoint, data))
            return None

        with patch.object(release.GitLab, "request", side_effect=request), patch.object(release.GitLab, "__init__", return_value=None) as init:
            # __init__ is mocked only to avoid retaining credentials in the object.
            with patch.object(release.GitLab, "base", "https://gitlab.example/api/v4", create=True):
                release.publish(self.root)
        init.assert_called_once_with(self.env["CI_API_V4_URL"], self.env["CI_JOB_TOKEN"])
        uploads = [c for c in calls if c[0] == "PUT"]
        self.assertEqual(len(uploads), 5)
        self.assertTrue(all("/packages/generic/pui/1.2.3/" in c[1] for c in uploads))
        payload = [c[2] for c in calls if c[0] == "POST"][0]
        self.assertEqual(payload["tag_name"], "v1.2.3")
        self.assertEqual(len(payload["assets"]["links"]), 5)
        self.assertIn("Current feature", payload["description"])
        self.assertNotIn("Future work", payload["description"])
        self.assertTrue(all("/jobs/" not in link["url"] for link in payload["assets"]["links"]))

    def test_publish_rejects_unprotected_tampered_wrong_commit_or_unsigned_required(self):
        directory = self.prepare()
        for env in ({"CI_COMMIT_REF_PROTECTED": "false"}, {"CI_COMMIT_SHA": "b" * 40}, {"PUI_REQUIRE_SIGNING": "true"}):
            with self.subTest(env=env), patch.dict(os.environ, env), patch.object(release.GitLab, "request") as request:
                with self.assertRaises(ValueError):
                    release.publish(self.root)
                request.assert_not_called()
        (directory / "Pui-1.2.3-macos-arm64-adhoc.zip").write_bytes(b"tampered")
        with patch.object(release.GitLab, "request") as request, self.assertRaises(ValueError):
            release.publish(self.root)
        request.assert_not_called()

    def test_workspace_symlinks_are_not_uploaded_even_with_matching_hash(self):
        directory = self.prepare()
        for name in ("release.json", "SHA256SUMS", "CHANGELOG.md", "Pui-1.2.3-macos-arm64-adhoc.zip"):
            path = directory / name
            moved = self.root / (name + ".outside")
            path.rename(moved)
            path.symlink_to(moved)
            with self.subTest(name=name), patch.object(release.GitLab, "request") as request:
                with self.assertRaisesRegex(ValueError, "symlinks"):
                    release.publish(self.root)
                request.assert_not_called()
            path.unlink()
            moved.rename(path)

    def test_retries_reuse_identical_assets_but_never_overwrite_different_content(self):
        self.prepare()
        with patch.object(release.GitLab, "request", return_value="different hash") as request:
            with self.assertRaisesRegex(ValueError, "immutable"):
                release.publish(self.root)
        self.assertEqual(request.call_count, 1)
        self.assertEqual(request.call_args.args[0], "GET")
        directory = self.root / "dist/releases"

        def existing(method, endpoint, *args, **kwargs):
            if "/packages/" in endpoint:
                return release.digest(directory / endpoint.rsplit("/", 1)[1])
            return None

        with patch.object(release.GitLab, "request", side_effect=existing) as request:
            release.publish(self.root)
        self.assertNotIn("PUT", [c.args[0] for c in request.call_args_list])

    def test_token_requests_reject_redirects_and_insecure_urls(self):
        for url in ("http://gitlab.example/api/v4", "https://user:password@gitlab.example/api/v4"):
            with self.assertRaises(ValueError):
                release.GitLab(url, "fake")
        self.assertIsNone(release.NoRedirect().redirect_request(None, None, 302, "", {}, "https://other.example"))
        request = release.Request("https://gitlab.example/api/v4/packages/file", headers={
            "JOB-TOKEN": "fake-secret", "PRIVATE-TOKEN": "fake-project", "Authorization": "Bearer fake", "Cookie": "session=fake"})
        redirect = release.DownloadRedirect()
        redirected = redirect.redirect_request(request, None, 302, "", {}, "https://storage.example/signed?key=fake")
        self.assertEqual(redirected.full_url, "https://storage.example/signed?key=fake")
        self.assertEqual(redirected.header_items(), [])
        for target in ("http://storage.example/file", "https://user:pass@storage.example/file"):
            self.assertIsNone(redirect.redirect_request(request, None, 302, "", {}, target))
        request.method = "PUT"
        self.assertIsNone(redirect.redirect_request(request, None, 302, "", {}, "https://storage.example/file"))


class TransportTests(unittest.TestCase):
    def response(self, body):
        import io
        return io.BytesIO(body)

    def test_get_hash_uses_streaming_download_opener_and_initial_token(self):
        api = release.GitLab("https://gitlab.example/api/v4", "fake-token")
        api.download_opener = Mock()
        api.download_opener.open.return_value = self.response(b"artifact contents")
        self.assertEqual(api.request("GET", "/packages/file", sha=True),
                         release.hashlib.sha256(b"artifact contents").hexdigest())
        request = api.download_opener.open.call_args.args[0]
        self.assertEqual(request.get_header("Job-token"), "fake-token")
        self.assertEqual(request.get_method(), "GET")

    def test_upload_stream_has_content_length_and_token_without_reading_whole_file(self):
        with tempfile.NamedTemporaryFile() as artifact:
            artifact.write(b"fake zip")
            artifact.flush()
            api = release.GitLab("https://gitlab.example/api/v4", "fake-token")
            api.opener = Mock()

            def upload(request, **kwargs):
                self.assertEqual(request.get_method(), "PUT")
                self.assertEqual(request.get_header("Job-token"), "fake-token")
                self.assertEqual(request.get_header("Content-length"), "8")
                self.assertEqual(request.data.read(), b"fake zip")
                return self.response(b'{"message":"201 Created"}')

            api.opener.open.side_effect = upload
            self.assertEqual(api.request("PUT", "/packages/file", Path(artifact.name), "application/octet-stream"),
                             {"message": "201 Created"})

    def test_project_tokens_use_private_token_header_without_changing_job_token_default(self):
        api = release.GitLab("https://gitlab.example/api/v4", "fake-project-token", "PRIVATE-TOKEN")
        api.opener = Mock()
        api.opener.open.return_value = self.response(b'{}')
        api.request("GET", "/projects/123")
        request = api.opener.open.call_args.args[0]
        self.assertEqual(request.get_header("Private-token"), "fake-project-token")
        self.assertIsNone(request.get_header("Job-token"))
        for header, token in (("Authorization", "fake"), ("PRIVATE-TOKEN", "")):
            with self.assertRaises(ValueError):
                release.GitLab("https://gitlab.example/api/v4", token, header)

    def test_only_explicit_404_is_missing_and_errors_do_not_leak_credentials_or_body(self):
        api = release.GitLab("https://gitlab.example/api/v4", "fake-token")
        api.opener = Mock()
        for status in (302, 403, 404, 500):
            api.opener.open.side_effect = release.HTTPError("https://secret-query.invalid?token=secret", status,
                                                          "sensitive body", {}, None)
            with self.assertRaises(ValueError) as error:
                api.request("GET", "/releases/v1.2.3", missing=True)
            self.assertNotIn("secret", str(error.exception))
            self.assertNotIn("sensitive", str(error.exception))
            self.assertIn(f"HTTP {status}", str(error.exception))
        with self.assertRaises(ValueError):
            api.request("GET", "/releases/v1.2.3")
        api.opener.open.side_effect = release.HTTPError("https://gitlab.example/api/v4/releases/v1.2.3", 404,
                                                      "missing", {}, None)
        self.assertIsNone(api.request("GET", "/releases/v1.2.3", missing=True))

    def test_notary_wrapper_requires_accepted_and_preserves_cli_failures(self):
        with tempfile.TemporaryDirectory() as temp:
            command = Path(temp) / "xcrun"
            command.write_text('#!/bin/bash\nprintf \'%s\\n\' "$NOTARY_REPLY"\nexit "$NOTARY_EXIT"\n')
            command.chmod(0o755)
            env = {**os.environ, "PATH": temp + ":/usr/local/bin:/usr/bin:/bin",
                   "PUI_SIGN_KEYCHAIN": "/tmp/fake", "PUI_NOTARY_PROFILE": "fake-profile"}
            for reply, code, success in (("Accepted", 0, True), ("Invalid", 0, False), ("In Progress", 1, False)):
                result = subprocess.run(["/bin/bash", str(ROOT / "scripts/notarize-macos.sh"), "/tmp/fake.zip"],
                                        env={**env, "NOTARY_REPLY": json.dumps({"status": reply}), "NOTARY_EXIT": str(code)},
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode == 0, success, result.stderr)


class SigningTests(unittest.TestCase):
    def test_signs_nested_code_inside_out_and_verifies_deep_only(self):
        with tempfile.TemporaryDirectory() as temp:
            app = Path(temp).resolve() / "Pui.app"
            framework = app / "Contents/Frameworks/Chromium Embedded Framework.framework"
            (framework / "Libraries").mkdir(parents=True)
            library = framework / "Libraries/libcef_sandbox.dylib"
            library.touch()
            for suffix in ("", " (GPU)", " (Renderer)", " (Plugin)", " (Alerts)"):
                (app / f"Contents/Frameworks/Pui Browser Helper{suffix}.app").mkdir()
            with patch.dict(os.environ, {"PUI_SIGN_IDENTITY": "Developer ID Application: Example (TEST)", "PUI_SIGN_KEYCHAIN": "/tmp/test-keychain"}), patch.object(signer.subprocess, "run") as run:
                signer.sign(app)
            commands = [c.args[0] for c in run.call_args_list]
            self.assertEqual(commands[0][-1], str(library))
            self.assertEqual(commands[1][-1], str(framework))
            self.assertEqual(commands[-2][-1], str(app))
            self.assertIn("--verify", commands[-1])
            for command in commands[:-1]:
                self.assertNotIn("--deep", command)
                self.assertIn("runtime", command)
                self.assertIn("--timestamp", command)
            entitlements = [c for c in commands if "--entitlements" in c]
            self.assertEqual(len(entitlements), 5)
            with (ROOT / "packaging/cef-helper-entitlements.plist").open("rb") as source:
                self.assertEqual(plistlib.load(source), {"com.apple.security.cs.allow-jit": True})

    def test_sign_failure_is_not_downgraded(self):
        with tempfile.TemporaryDirectory() as temp, patch.dict(os.environ, {"PUI_SIGN_IDENTITY": "Developer ID Application: Example (TEST)"}), patch.object(signer.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "codesign")) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                signer.sign(Path(temp))
            self.assertEqual(run.call_count, 1)


class BuildPolicyTests(unittest.TestCase):
    def test_every_shell_script_parses_individually(self):
        paths = [ROOT / "scripts" / name for name in ("bundle-macos.sh", "package-macos.sh", "notarize-macos.sh")]
        paths += sorted((ROOT / "scripts/ci").glob("*.sh"))
        for path in paths:
            with self.subTest(path=path):
                subprocess.run(["/bin/bash", "-n", str(path)], check=True)

    def run_build(self, env, fake_failure="bundle-macos.sh"):
        """Execute the real wrapper with mocked platform commands; no Apple/network calls."""
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "scripts/ci").mkdir(parents=True)
            (root / "scripts/ci/build-macos.sh").write_text((ROOT / "scripts/ci/build-macos.sh").read_text())
            bin_dir = root / "bin"
            bin_dir.mkdir()
            log = root / "commands"
            # Retain the real Bash only to execute the wrapper itself. Its child
            # bash/python/security calls are bounded command stubs.
            stub = '''#!/bin/bash
printf '%s\\n' "$(basename "$0") $*" >> "$COMMAND_LOG"
case "$(basename "$0")" in
 python3) if [[ "${1:-}" == -c ]]; then /usr/bin/python3 "$@"; fi ;;
 security)
   if [[ "$*" == 'list-keychains -d user' ]]; then echo '"/tmp/original.keychain-db"'; fi
   if [[ "$*" == *"$FAIL_COMMAND"* && -n "$FAIL_COMMAND" ]]; then exit 17; fi ;;
 bash) if [[ "$*" == *"$FAIL_COMMAND"* ]]; then exit 17; fi ;;
esac
'''
            for command in ("python3", "security", "bash", "xcrun", "ditto", "spctl"):
                path = bin_dir / command
                path.write_text(stub)
                path.chmod(0o755)
            cert = root / "certificate.p12"
            cert.touch()
            environment = {"PATH": str(bin_dir) + ":/usr/bin:/bin", "COMMAND_LOG": str(log),
                           "FAIL_COMMAND": fake_failure, "PUI_SIGN_P12": str(cert), **env}
            result = subprocess.run(["/bin/bash", str(root / "scripts/ci/build-macos.sh")], env=environment, capture_output=True, text=True)
            return result, log.read_text() if log.exists() else ""

    def credentials(self):
        return {"PUI_SIGN_IDENTITY": "Developer ID Application: Example (TEST)", "PUI_SIGN_PASSWORD": "fake",
                "PUI_APPLE_ID": "example@example.invalid", "PUI_APPLE_TEAM_ID": "TEST", "PUI_APPLE_APP_PASSWORD": "fake"}

    def test_mr_and_branch_never_import_certificates(self):
        for env in ({}, {"CI_COMMIT_TAG": "v1.2.3", "CI_COMMIT_REF_PROTECTED": "false"}):
            result, log = self.run_build({**self.credentials(), **env})
            self.assertEqual(result.returncode, 17, result.stderr)
            self.assertNotIn("security ", log)
            self.assertIn("bundle-macos.sh", log)

    def test_partial_credentials_fail_closed(self):
        result, log = self.run_build({"CI_COMMIT_TAG": "v1.2.3", "CI_COMMIT_REF_PROTECTED": "true"})
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("bundle-macos.sh", log)

    def test_required_signing_refuses_missing_credentials(self):
        result, log = self.run_build({"CI_COMMIT_TAG": "v1.2.3", "CI_COMMIT_REF_PROTECTED": "true",
                                      "PUI_REQUIRE_SIGNING": "true", "PUI_SIGN_P12": ""})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires signing credentials", result.stderr)
        self.assertNotIn("bundle-macos.sh", log)

    def test_base64_file_is_decoded_inside_ephemeral_directory(self):
        import base64
        with tempfile.NamedTemporaryFile() as certificate:
            certificate.write(base64.b64encode(b"fake cert"))
            certificate.flush()
            result, log = self.run_build({**self.credentials(), "CI_COMMIT_TAG": "v1.2.3",
                                          "CI_COMMIT_REF_PROTECTED": "true", "PUI_SIGN_P12": "",
                                          "PUI_SIGN_P12_BASE64": certificate.name})
        self.assertEqual(result.returncode, 17, result.stderr)
        self.assertIn("/certificate.p12 -k", log)
        self.assertIn("delete-keychain", log)

    def test_conflicting_certificate_sources_fail_before_import(self):
        with tempfile.NamedTemporaryFile() as certificate:
            result, log = self.run_build({**self.credentials(), "CI_COMMIT_TAG": "v1.2.3",
                                          "CI_COMMIT_REF_PROTECTED": "true", "PUI_SIGN_P12_BASE64": certificate.name})
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("security import", log)

    def test_failure_restores_keychain_search_list_and_deletes_ephemeral_keychain(self):
        for failure in ("bundle-macos.sh", "import"):
            result, log = self.run_build({**self.credentials(), "CI_COMMIT_TAG": "v1.2.3", "CI_COMMIT_REF_PROTECTED": "true"}, failure)
            self.assertEqual(result.returncode, 17, result.stderr)
            self.assertIn("delete-keychain", log)
            self.assertIn("list-keychains -d user -s /tmp/original.keychain-db", log)
            self.assertNotIn("package-macos.sh", log)

    def test_debug_trace_is_rejected_before_credentials_are_used(self):
        result, log = self.run_build({"CI_DEBUG_TRACE": "true", **self.credentials()})
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("security ", log)


if __name__ == "__main__":
    unittest.main()
