#!/usr/bin/env python3
"""Validate stable releases, describe artifacts, and publish using CI_JOB_TOKEN.

Python 3.11+; no third-party dependencies. Never logs HTTP bodies or credentials.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib
from urllib.error import HTTPError
from urllib.parse import quote, urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener

ROOT = Path(__file__).resolve().parent.parent
STABLE = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\Z")


def version(root=ROOT):
    value = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
    if not STABLE.fullmatch(value):
        raise ValueError("Cargo version must be a stable semantic version (X.Y.Z).")
    return value


def notes(root, value):
    text = (root / "CHANGELOG.md").read_text()
    headings = list(re.finditer(r"^## \[([^\]]+)\][^\n]*$", text, re.MULTILINE))
    matches = [(i, h) for i, h in enumerate(headings) if h.group(1) == value]
    if len(matches) != 1:
        raise ValueError(f"CHANGELOG.md must contain exactly one ## [{value}] section.")
    i, heading = matches[0]
    body = text[heading.end():headings[i + 1].start() if i + 1 < len(headings) else len(text)].strip()
    if not body:
        raise ValueError("Release changelog section is empty.")
    return body + "\n"


def check(root=ROOT, tag=None):
    value = version(root)
    if tag and tag != f"v{value}":
        raise ValueError(f"Tag must be v{value}, matching Cargo.toml.")
    notes(root, value)
    return value


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def prepare(root=ROOT):
    value = check(root, os.environ.get("CI_COMMIT_TAG"))
    signing = os.environ.get("PUI_SIGNING_STATUS", "ad-hoc")
    if signing not in ("ad-hoc", "developer-id-notarized"):
        raise ValueError("Unknown signing status.")
    directory = root / "dist/releases"
    suffix = "-adhoc" if signing == "ad-hoc" else ""
    name = f"Pui-{value}-macos-arm64{suffix}"
    files = {f"{name}.{ext}": digest(directory / f"{name}.{ext}") for ext in ("zip", "dmg")}
    commit = os.environ.get("CI_COMMIT_SHA") or subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    runtime = root / "dist/Pui.app/Contents/Resources/runtime/manifest.json"
    runtime_data = json.loads(runtime.read_text())
    manifest = {"version": value, "tag": f"v{value}", "commit": commit,
                "architecture": "arm64", "signing": signing, "files": files,
                "runtime": runtime_data}
    (directory / "release.json").write_text(json.dumps(manifest, indent=2) + "\n")
    body = notes(root, value)
    if signing == "ad-hoc":
        body += "\n**Ad-hoc build:** not Developer ID signed or notarized. macOS Gatekeeper may block opening it.\n"
    else:
        body += "\nDeveloper ID signed and Apple-notarized Apple Silicon build.\n"
    body += "\nIncludes Pi and Node.js; no separate runtime installation is required. Configure a model provider to begin. Intel Macs are not supported by these assets.\n"
    (directory / "CHANGELOG.md").write_text(body)
    for filename in ("release.json", "CHANGELOG.md"):
        files[filename] = digest(directory / filename)
    (directory / "SHA256SUMS").write_text("".join(f"{sha}  {name}\n" for name, sha in sorted(files.items())))


class NoRedirect(HTTPRedirectHandler):
    # API mutations may not redirect; downloads use a credential-stripping handler.
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class DownloadRedirect(HTTPRedirectHandler):
    """Follow package/object-store GET redirects without forwarding credentials."""
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        parsed = urlsplit(newurl)
        if req.get_method() != "GET" or parsed.scheme != "https" or parsed.username or parsed.password:
            return None
        redirected = super().redirect_request(req, fp, code, msg, headers, newurl)
        if redirected is None:
            return None
        # Reconstruct rather than blacklist headers: never forward JOB-TOKEN,
        # Authorization, cookies, or future authentication headers, even same-origin.
        return Request(redirected.full_url, method="GET", unverifiable=True)


class GitLab:
    def __init__(self, base, token):
        if urlsplit(base).scheme != "https" or urlsplit(base).username:
            raise ValueError("GitLab API must be an HTTPS URL without embedded credentials.")
        self.base, self.token = base.rstrip("/"), token
        self.opener = build_opener(NoRedirect())
        self.download_opener = build_opener(DownloadRedirect())

    def request(self, method, endpoint, data=None, content_type="application/json", missing=False, sha=False):
        headers = {"JOB-TOKEN": self.token, "Content-Type": content_type}
        if isinstance(data, Path):
            headers["Content-Length"] = str(data.stat().st_size)
            with data.open("rb") as source:
                return self._send(method, endpoint, source, headers, missing, sha)
        if data is not None:
            data = json.dumps(data).encode()
        return self._send(method, endpoint, data, headers, missing, sha)

    def _send(self, method, endpoint, data, headers, missing, sha):
        request = Request(self.base + endpoint, data=data, headers=headers, method=method)
        try:
            opener = self.download_opener if sha and method == "GET" else self.opener
            with opener.open(request, timeout=300) as response:
                if sha:
                    result = hashlib.sha256()
                    for chunk in iter(lambda: response.read(1024 * 1024), b""):
                        result.update(chunk)
                    return result.hexdigest()
                body = response.read(2 * 1024 * 1024)
                return json.loads(body) if body else {}
        except HTTPError as error:
            if missing and error.code == 404:
                return None
            raise ValueError(f"GitLab {method} failed (HTTP {error.code}); no release was overwritten.") from None


def publish(root=ROOT):
    tag = os.environ["CI_COMMIT_TAG"]
    if os.environ.get("CI_COMMIT_REF_PROTECTED") != "true":
        raise ValueError("Publishing requires a protected version tag.")
    value = check(root, tag)
    directory = root / "dist/releases"
    manifest = json.loads((directory / "release.json").read_text())
    if (manifest.get("version"), manifest.get("tag"), manifest.get("commit"), manifest.get("architecture")) != (
            value, tag, os.environ["CI_COMMIT_SHA"], "arm64"):
        raise ValueError("Artifact provenance does not match this release pipeline.")
    signing = manifest.get("signing")
    if signing not in ("ad-hoc", "developer-id-notarized"):
        raise ValueError("Unknown artifact signing status.")
    if os.environ.get("PUI_REQUIRE_SIGNING", "false") == "true" and signing != "developer-id-notarized":
        raise ValueError("This project requires notarized releases.")
    suffix = "-adhoc" if signing == "ad-hoc" else ""
    expected = {f"Pui-{value}-macos-arm64{suffix}.{ext}" for ext in ("zip", "dmg")}
    if set(manifest["files"]) != expected:
        raise ValueError("Unexpected release assets.")
    sums = {}
    for line in (directory / "SHA256SUMS").read_text().splitlines():
        sha, name = line.split("  ", 1)
        if name not in expected | {"release.json", "CHANGELOG.md"} or name in sums:
            raise ValueError("Unexpected checksum filename.")
        if digest(directory / name) != sha:
            raise ValueError("Artifact checksum mismatch.")
        sums[name] = sha
    if set(sums) != expected | {"release.json", "CHANGELOG.md"} or any(sums[name] != sha for name, sha in manifest["files"].items()):
        raise ValueError("Incomplete artifact checksums.")
    api = GitLab(os.environ["CI_API_V4_URL"], os.environ["CI_JOB_TOKEN"])
    project = f"/projects/{quote(os.environ['CI_PROJECT_ID'], safe='')}"
    package = f"{project}/packages/generic/pui/{value}"
    assets = []
    for name in sorted(set(sums) | {"SHA256SUMS"}):
        endpoint = f"{package}/{quote(name, safe='')}"
        local = digest(directory / name)
        existing = api.request("GET", endpoint, missing=True, sha=True)
        if existing is not None and existing != local:
            raise ValueError(f"Published asset differs: {name}. Bump the version; assets are immutable.")
        if existing is None:
            api.request("PUT", endpoint, directory / name, "application/octet-stream")
        assets.append({"name": name, "url": api.base + endpoint, "link_type": "package",
                       "direct_asset_path": "/" + name})
    body = {"name": f"Pui {value}", "tag_name": tag,
            "description": (directory / "CHANGELOG.md").read_text(), "assets": {"links": assets}}
    endpoint = f"{project}/releases"
    existing = api.request("GET", endpoint + "/" + quote(tag, safe=""), missing=True)
    if existing is not None:
        current = {link["name"]: link["url"] for link in existing.get("assets", {}).get("links", [])}
        if existing.get("description") != body["description"] or current != {a["name"]: a["url"] for a in assets}:
            raise ValueError("Release already exists with different notes or assets; refusing to overwrite.")
    else:
        api.request("POST", endpoint, body)
    print(f"Published GitLab release {tag} ({signing}).")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "version", "prepare", "publish"))
    args = parser.parse_args()
    try:
        if args.command == "check":
            print(check(tag=os.environ.get("CI_COMMIT_TAG")))
        elif args.command == "version":
            print(version())
        elif args.command == "prepare":
            prepare()
        else:
            publish()
    except (ValueError, KeyError, OSError) as error:
        # Do not interpolate network exception URLs, environment values, or server bodies.
        if isinstance(error, ValueError):
            print(f"Release error: {error}", file=sys.stderr)
        else:
            print("Release error: missing input or failed I/O; inspect CI configuration and artifacts.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
