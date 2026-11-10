#!/usr/bin/env python3
"""Copy the exact CEF build output and sandbox helpers into Pui.app."""
import json
import os
from pathlib import Path
import plistlib
import shutil
import sys

app, target, profile = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
outputs = sorted((target / profile / "build").glob("cef-dll-sys-*/output"), key=lambda p: p.stat().st_mtime, reverse=True)
cef = None
for output in outputs:
    for line in output.read_text().splitlines():
        if line.startswith("cargo::metadata=CEF_DIR="):
            candidate = Path(line.split("=", 2)[2])
            if (candidate / "archive.json").is_file():
                cef = candidate
                break
    if cef:
        break
if not cef:
    raise SystemExit("Cannot locate the pinned CEF build output; build pui and pui-browser-helper first.")
frameworks = app / "Contents/Frameworks"
frameworks.mkdir(parents=True, exist_ok=True)
source = cef / "Release/Chromium Embedded Framework.framework"
if not source.is_dir():
    source = cef / "Chromium Embedded Framework.framework"
if not source.is_dir():
    raise SystemExit(f"CEF framework missing: {cef}")
# Overlay copies retain framework symlinks. No downloads or removal of user data.
shutil.copytree(source, frameworks / source.name, symlinks=True, dirs_exist_ok=True)
for suffix in ["", " (GPU)", " (Renderer)", " (Plugin)", " (Alerts)"]:
    name = f"Pui Browser Helper{suffix}"
    contents = frameworks / f"{name}.app/Contents"
    binary = contents / "MacOS" / name
    binary.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(target / profile / "pui-browser-helper", binary)
    with (contents / "Info.plist").open("wb") as out:
        plistlib.dump({"CFBundleExecutable": name, "CFBundleIdentifier": "dev.pi.pui.browser-helper" + suffix.replace(" (", ".").replace(")", "").lower(), "CFBundleName": name, "CFBundlePackageType": "APPL", "CFBundleVersion": "1", "LSUIElement": True, "LSMinimumSystemVersion": "12.0"}, out)
resources = app / "Contents/Resources"
shutil.copy2(cef / "LICENSE.txt", resources / "CEF-LICENSE.txt")
shutil.copy2(cef / "CREDITS.html", resources / "CEF-CREDITS.html")
shutil.copy2(cef / "archive.json", resources / "CEF-archive.json")
print(f"Bundled CEF from {cef}")
