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
    runtime = app / "Contents/Resources/runtime"
    if runtime.exists():
        # Match file magic, not suffix: QuickJS .so extensions are WASM, while
        # native npm addons and extensionless rg/fd/Node are Mach-O code.
        macho = {b"\xcf\xfa\xed\xfe", b"\xce\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xfe\xed\xfa\xce", b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca"}
        node = runtime / "node/bin/node"
        for binary in sorted(runtime.rglob("*")):
            if binary.is_symlink() or not binary.is_file():
                continue
            with binary.open("rb") as source:
                header = source.read(4)
            if header not in macho:
                continue
            extra = []
            if binary == node:
                extra = ["--entitlements", str(Path(__file__).resolve().parent.parent / "packaging/node-entitlements.plist")]
            subprocess.run(args + extra + [str(binary)], check=True)
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
