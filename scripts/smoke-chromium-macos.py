#!/usr/bin/env python3
"""Opt-in bundled DEBUG smoke: CEF pixels, not egui's framebuffer. No provider calls.

Usage: python3 scripts/smoke-chromium-macos.py /tmp/Pui.app/Contents/MacOS/pui URL [responsive|mobile]
Diagnostics and the screenshot stay in a fresh temporary directory; nothing is uploaded.
Use a local fixture for repeatable tests. No browser login or consent actions are clicked.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    binary = Path(sys.argv[1]).resolve(strict=True)
    url = sys.argv[2]
    device = sys.argv[3] if len(sys.argv) > 3 else "responsive"
    output = Path(tempfile.mkdtemp(prefix="pui-chromium-smoke-"))
    report, screenshot = output / "page.json", output / "page.png"
    env = os.environ.copy()
    env["PUI_BROWSER_SMOKE_REPORT"] = str(report)
    with (output / "app.log").open("w") as log:
        app = subprocess.Popen([
            str(binary), "--demo", "--cwd", str(output), "--open", url,
            "--browser-console", "--browser-device", device,
        ], env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 40
            state = {}
            while time.monotonic() < deadline:
                if app.poll() is not None:
                    raise RuntimeError(f"App exited early: {app.returncode}; {output / 'app.log'}")
                try:
                    state = json.loads(report.read_text())
                except (FileNotFoundError, json.JSONDecodeError):
                    time.sleep(0.1)
                    continue
                if state.get("error"):
                    raise RuntimeError(f"{state['error']}; {output}")
                if state.get("title") and not state.get("loading") and screenshot.is_file():
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError(f"Page never completed: {state}; {output}")
            child, container = state["child"], state["container"]
            assert child["x"] == child["y"] == 0, state
            assert child["width"] == container["width"] > 0, state
            assert child["height"] == container["height"] > 0, state
            assert screenshot.read_bytes()[:8] == b"\x89PNG\r\n\x1a\n", "Not a CEF PNG"
            assert screenshot.stat().st_size > 1000, "Unexpectedly empty screenshot"
            print(f"PASS: {state['title']} — {state['location']}")
            print(f"Native page constrained to {child['width']} × {child['height']}")
            print(f"Chromium screenshot: {screenshot}\nDiagnostics: {report}")
        finally:
            if app.poll() is None:
                app.terminate()  # Only the exact demo process this script started.
                try:
                    app.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    app.kill()
                    app.wait(timeout=5)


if __name__ == "__main__":
    main()
