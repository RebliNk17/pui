#!/usr/bin/env python3
"""Fetch the CEF version pinned by Cargo.toml, verifying the official archive hash.
No installs outside target/, privilege changes, or insecure TLS fallback.
"""
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import sys

version = "156.0.26+g8ea0c73+chromium-156.0.8078.88"
archives = {
    "arm64": ("macosarm64", "ca4fafe7a9c52d9f28200bb8713971a9662b891c", 134979837),
    "x86_64": ("macosx64", "f0f47aa3cdefa7eea54f2e8c6e73c68b4db125f3", 141699633),
}
if platform.system() != "Darwin" or platform.machine() not in archives:
    raise SystemExit("CEF fetch currently supports macOS arm64/x86_64 only.")
arch, checksum, size = archives[platform.machine()]
name = f"cef_binary_{version}_{arch}_minimal.tar.bz2"
root = Path(sys.argv[1] if len(sys.argv) > 1 else "target/cef-runtime").resolve()
root.mkdir(parents=True, exist_ok=True)
destination = root / name.removesuffix(".tar.bz2")
marker = destination / "archive.json"
if marker.is_file():
    metadata = json.loads(marker.read_text())
    if metadata.get("name") != name or metadata.get("sha1") != checksum:
        raise SystemExit("Existing CEF metadata does not match the pinned distribution.")
else:
    archive = root / name
    if not archive.is_file():
        subprocess.run(["curl", "--fail", "--location", "--retry", "2", f"https://cef-builds.spotifycdn.com/{name.replace('+', '%2B')}", "--output", str(archive)], check=True)
    digest = hashlib.sha1()
    with archive.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    if archive.stat().st_size != size or digest.hexdigest() != checksum:
        raise SystemExit(f"CEF archive verification failed: {archive}; retained for inspection.")
    subprocess.run(["tar", "-xjf", str(archive), "-C", str(root)], check=True)
    marker.write_text(json.dumps({"name": name, "sha1": checksum, "size": size, "type": "minimal", "last_modified": "2026-11-07T00:00:00Z"}))
framework = destination / "Release/Chromium Embedded Framework.framework"
if not framework.is_dir():
    raise SystemExit("CEF extraction is incomplete; framework missing.")
print(destination)
