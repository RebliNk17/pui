#!/usr/bin/env python3
"""Sign nested Mach-O code inside-out; never use --deep for signing."""
import os
from pathlib import Path
import subprocess
import sys


def sign(app):
    app = app.resolve()
    identity = os.environ.get("PUI_SIGN_IDENTITY", "-")
    if not identity or (identity != "-" and not identity.startswith("Developer ID Application:")):
        raise ValueError("Use a Developer ID Application identity, or '-' for ad-hoc signing.")
    args = ["codesign", "--force", "--sign", identity]
    if identity != "-":
        args += ["--options", "runtime", "--timestamp"]
        if os.environ.get("PUI_SIGN_KEYCHAIN"):
            args += ["--keychain", os.environ["PUI_SIGN_KEYCHAIN"]]
    framework = app / "Contents/Frameworks/Chromium Embedded Framework.framework"
    # CEF has nested Vulkan/sandbox dynamic libraries; signing just the framework
    # leaves those ad-hoc signed and fails notarization.
    for library in sorted(framework.rglob("*.dylib")):
        if not library.is_symlink():
            subprocess.run(args + [str(library)], check=True)
    subprocess.run(args + [str(framework)], check=True)
    entitlements = Path(__file__).resolve().parent.parent / "packaging/cef-helper-entitlements.plist"
    helpers = sorted((app / "Contents/Frameworks").glob("Pui Browser Helper*.app"))
    if len(helpers) != 5:
        raise ValueError("Expected all five sandboxed CEF helper bundles.")
    for helper in helpers:
        # Chromium's V8 JIT entitlement belongs on helpers, not the Pui UI process.
        subprocess.run(args + ["--entitlements", str(entitlements), str(helper)], check=True)
    subprocess.run(args + [str(app)], check=True)
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)


if __name__ == "__main__":
    sign(Path(sys.argv[1]))
