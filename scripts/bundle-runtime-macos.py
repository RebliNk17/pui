#!/usr/bin/env python3
"""Assemble the immutable Pi/Node runtime (macOS arm64, no global installs).

Official archives use checked-in SHA-256 pins. Pi and its npm dependency graph
use npm ci with a checked-in integrity lock and lifecycle scripts disabled.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parent.parent
PACKAGE = ROOT / "packaging/runtime"


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def fetch(pin, cache):
    name = pin["url"].rsplit("/", 1)[1]
    archive = cache / name
    if not archive.exists():
        partial = archive.with_suffix(archive.suffix + ".partial")
        subprocess.run(["curl", "--proto", "=https", "--proto-redir", "=https", "--fail", "--location",
                        "--retry", "2", "--max-time", "180", pin["url"], "-o", str(partial)], check=True)
        if sha256(partial) != pin["sha256"]:
            raise ValueError(f"Archive checksum mismatch: {name}")
        partial.rename(archive)
    if sha256(archive) != pin["sha256"]:
        raise ValueError(f"Cached archive checksum mismatch: {name}")
    return archive


def extract(archive, destination):
    # Reject paths/links/devices before using the system tar (Python 3.9+).
    destination.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive) as source:
        members = source.getmembers()
        by_path = {Path(member.name): member for member in members}
        if len(by_path) != len(members):
            raise ValueError("Runtime archive contains duplicate member paths.")
        for member in members:
            path = Path(member.name)
            if path.is_absolute() or ".." in path.parts or not (member.isfile() or member.isdir() or member.issym() or member.islnk()):
                raise ValueError("Unsafe runtime archive member.")
            if member.islnk():
                raise ValueError("Runtime archive hardlinks are not supported.")
            if member.issym():
                link = Path(member.linkname)
                target = (destination / path.parent / link).resolve()
                if link.is_absolute() or not target.is_relative_to(destination.resolve()):
                    raise ValueError("Unsafe runtime archive link.")
                # Official Node archives use only npm/npx/corepack bin links.
                # Reject link chains and members beneath links rather than
                # interpreting tar's virtual symlink graph during extraction.
                virtual_target = Path(os.path.normpath(str(path.parent / link)))
                target_member = by_path.get(virtual_target)
                if target_member is None or not target_member.isfile():
                    raise ValueError("Runtime archive symlink must point directly to a regular member.")
        links = [Path(item.name) for item in members if item.issym()]
        for member in members:
            if any(link in Path(member.name).parents for link in links):
                raise ValueError("Runtime archive member is nested below a symlink.")
    subprocess.run(["tar", "-xzf", str(archive), "-C", str(destination)], check=True)
    roots = list(destination.iterdir())
    if len(roots) != 1 or not roots[0].is_dir():
        raise ValueError("Expected one archive root directory.")
    return roots[0]


def inventory(node_modules, output):
    packages = []
    overrides = json.loads((PACKAGE / "notice-overrides.json").read_text())
    for manifest in sorted(node_modules.rglob("package.json")):
        # package.json examples/fixtures are not installed package roots.
        parent = manifest.parent
        if parent.parent.name != "node_modules" and not (
                parent.parent.name.startswith("@") and parent.parent.parent.name == "node_modules"):
            continue
        data = json.loads(manifest.read_text())
        licenses = []
        for file in sorted(parent.iterdir()):
            if file.is_file() and file.name.lower().startswith(("license", "licence", "copying", "notice", "copyright")):
                relative = file.relative_to(node_modules).as_posix()
                licenses.append(relative)
        supplement = overrides.get(data.get("name"), {})
        supplements = supplement.get("files", [])
        for file in supplements:
            if not (PACKAGE / "notices" / file).is_file():
                raise ValueError("Missing checked-in supplemental notice.")
        if not licenses and not supplements:
            raise ValueError(f"Missing license/notice file for installed package {data.get('name')}.")
        packages.append({"name": data["name"], "version": data["version"], "license": data.get("license"),
                         "notices": licenses, "supplemental_notices": supplements,
                         "attribution": supplement.get("attribution")})
    output.write_text(json.dumps(packages, indent=2) + "\n")
    return packages


def build(app, target):
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise ValueError("Bundled Pi runtime currently supports native macOS arm64 only.")
    cache = target.resolve() / "agent-runtime-cache"
    cache.mkdir(parents=True, exist_ok=True)
    pins = json.loads((PACKAGE / "pins.json").read_text())
    runtime = app.resolve() / "Contents/Resources/runtime"
    runtime.mkdir(parents=True, exist_ok=False)
    (runtime / "bin").mkdir()
    (runtime / "notices").mkdir()
    # Downloaded archives are cached; always extract/npm-ci afresh into staging.
    # This avoids copying Jiti caches, user auth, or mutated executable installs.
    for name in ("node", "rg", "fd"):
        archive = fetch(pins[name], cache)
        with tempfile.TemporaryDirectory(prefix="pui-runtime-extract-") as temp:
            source = extract(archive, Path(temp))
            if name == "node":
                destination = runtime / "node"
                destination.mkdir()
                (destination / "bin").mkdir()
                for file in ("node", "npm", "npx"):
                    shutil.copy2(source / "bin" / file, destination / "bin" / file, follow_symlinks=False)
                (destination / "lib/node_modules").mkdir(parents=True)
                # Node's official distribution provides npm + npx, needed by Pi's
                # resource/package operations without a separate npm installation.
                shutil.copytree(source / "lib/node_modules/npm", destination / "lib/node_modules/npm", symlinks=True)
                for file in ("LICENSE", "README.md"):
                    shutil.copy2(source / file, runtime / "notices" / f"Node-{file}")
            else:
                shutil.copy2(source / name, runtime / "bin" / name)
                for file in source.iterdir():
                    if file.is_file() and file.name.upper().startswith(("LICENSE", "COPYING", "NOTICE", "UNLICENSE")):
                        shutil.copy2(file, runtime / "notices" / f"{name}-{file.name}")
    node = runtime / "node/bin/node"
    pi = runtime / "pi"
    pi.mkdir()
    for file in ("package.json", "package-lock.json"):
        shutil.copy2(PACKAGE / file, pi / file)
    env = {**os.environ, "PATH": str(node.parent) + os.pathsep + os.environ.get("PATH", ""),
           "npm_config_update_notifier": "false"}
    subprocess.run([str(node), str(runtime / "node/lib/node_modules/npm/bin/npm-cli.js"), "ci", "--prefix", str(pi),
                    "--omit=dev", "--ignore-scripts", "--no-audit", "--no-fund", "--prefer-offline"], env=env, check=True)
    # No build-machine paths may survive as absolute or escaping npm links.
    for link in runtime.rglob("*"):
        if link.is_symlink() and (Path(os.readlink(link)).is_absolute()
                                  or not link.resolve().is_relative_to(runtime)):
            raise ValueError("Runtime contains a non-relocatable symlink.")
    # Remove foreign native prebuilds, not the notices/JS fallbacks. They cannot
    # execute on Apple Silicon and must not be submitted as unsigned PE/ELF code.
    for native in (pi / "node_modules").rglob("*.node"):
        if "darwin-arm64" not in native.parts:
            native.unlink()
    shutil.copy2(PACKAGE / "pi", runtime / "bin/pi")
    (runtime / "bin/pi").chmod(0o755)
    for name in ("node", "npm", "npx"):
        (runtime / "bin" / name).symlink_to(f"../node/bin/{name}")
    for link in runtime.rglob("*"):
        if link.is_symlink() and (not link.exists() or not link.resolve().is_relative_to(runtime)):
            raise ValueError("Runtime contains a broken or escaping symlink.")
    shutil.copytree(PACKAGE / "notices", runtime / "notices/supplemental")
    packages = inventory(pi / "node_modules", runtime / "notices/pi-packages.json")
    inventory(runtime / "node/lib/node_modules", runtime / "notices/npm-packages.json")
    (runtime / "notices/README.txt").write_text(
        "Pui bundled runtimes\n\nNode.js, npm and Pi package notices are retained in their original installed locations.\n"
        "pi-packages.json and npm-packages.json identify each package/version and its original license files.\n"
        "Node's combined LICENSE and ripgrep/fd licenses are in this directory.\n"
        "Runtime code is immutable; provider credentials, sessions and settings remain in the user's Pi directory.\n")
    manifest = {"schema": 1, "platform": "darwin-arm64", "node": pins["node"]["version"],
                "pi": next(p["version"] for p in packages if p["name"] == "@earendil-works/pi-coding-agent"),
                "archives": pins, "npm_lock_sha256": sha256(PACKAGE / "package-lock.json")}
    (runtime / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"Bundled Node {manifest['node']} and Pi {manifest['pi']} ({len(packages)} npm packages).")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", type=Path)
    parser.add_argument("target", nargs="?", type=Path, default=Path("target"))
    args = parser.parse_args()
    build(args.app, args.target)


if __name__ == "__main__":
    main()
