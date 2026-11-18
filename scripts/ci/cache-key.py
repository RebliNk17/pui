#!/usr/bin/env python3
"""Write CI cache keys before restore; never read environment secrets.

The download/build key covers ABI/tooling/dependencies, not app source. Cargo
fingerprints still decide what must rebuild after checkout. A daily build cache
suffix limits immutable snapshots to one per branch/environment per UTC day.
Compatible fallback snapshots never cross branches or dependency fingerprints.
"""
from datetime import datetime, timezone
import hashlib
import os
from pathlib import Path
import subprocess


def fingerprints(root, sdk, arch, system):
    toolchain = hashlib.sha256()
    for value in ("cache-v1", sdk, arch, system):
        toolchain.update(value.encode() + b"\0")
    dependencies = toolchain.copy()
    paths = [root / "Cargo.toml", root / "Cargo.lock", root / ".github/workflows/macos.yml"]
    paths += sorted((root / "scripts/ci").glob("*"))
    paths += [root / "scripts/fetch-chromium-macos.py", root / "scripts/bundle-runtime-macos.py"]
    paths += sorted((root / "packaging/runtime").rglob("*"))
    paths += sorted((root / "vendor").rglob("*.rs"))
    paths += sorted((root / "vendor").rglob("Cargo.toml"))
    for path in paths:
        if path.is_file():
            dependencies.update(path.relative_to(root).as_posix().encode() + b"\0")
            dependencies.update(path.read_bytes() + b"\0")
    return toolchain.hexdigest(), dependencies.hexdigest()


def main():
    root = Path(__file__).resolve().parent.parent.parent
    sdk = subprocess.check_output(["xcodebuild", "-version"], text=True)
    system = subprocess.check_output(["sw_vers", "-productVersion"], text=True)
    config = (root / ".github/workflows/macos.yml").read_text()
    # Include the checked-in Rust pin in the toolchain key, not unrelated edits.
    rust = next(line.strip() for line in config.splitlines() if line.strip().startswith("RUST_VERSION:"))
    toolchain, dependencies = fingerprints(root, sdk + rust, os.uname().machine, system)
    output = root / ".ci-cache"
    output.mkdir(exist_ok=True)
    for name, value in (("toolchain", toolchain), ("dependencies", dependencies),
                        ("day", datetime.now(timezone.utc).strftime("%Y-%m-%d"))):
        (output / name).write_text(value + "\n")
    print("Cache fingerprints prepared (Xcode, macOS, Rust, Cargo/runtime locks and pins, CI scripts and vendored Rust).")


if __name__ == "__main__":
    main()
