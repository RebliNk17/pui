#!/usr/bin/env python3
"""Run an owned bundled DEBUG app against an isolated localhost picker fixture.
No installed app, provider, external site, or existing helper process is touched.
Usage: python3 scripts/smoke-browser-picker-macos.py /tmp/Pui.app
"""
import http.server
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time


def main():
    app_path = Path(sys.argv[1]).resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix="pui-picker-acceptance-"))
    hits = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            hits.append(self.path)
            body = b'''<!doctype html><title>Pui picker local acceptance</title>
<style>body{margin:0}h1{position:absolute;left:20px;top:15px;width:400px;height:70px;margin:0;font-size:24px}</style>
<h1 id="public-title">Public picker fixture</h1>
<form style="position:absolute;top:150px"><input id="secret" type="password" value="NEVER_EXTRACT"></form>
<script>
for(const n of ['click','mousedown','mouseup','pointerdown','pointerup'])document.addEventListener(n,()=>fetch('/leaked-'+n));
// Main-world overrides must not run in the inspector's isolated world.
const poisoned=()=>{fetch('/leaked-page-getter');throw Error('page override');};
Element.prototype.matches=poisoned;Element.prototype.getBoundingClientRect=poisoned;
CharacterData.prototype.substringData=poisoned;window.getComputedStyle=poisoned;
</script>'''
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.end_headers()
            self.wfile.write(body)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    home = root / "home"
    home.mkdir()
    env = os.environ.copy()
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / "config"),
               PUI_BROWSER_PICKER_SMOKE="1", PUI_BROWSER_SMOKE_REPORT=str(root / "page.json"))
    with (root / "app.log").open("w") as log:
        app = subprocess.Popen([
            str(app_path / "Contents/MacOS/pui"), "--demo", "--cwd", str(root),
            "--open", f"http://127.0.0.1:{server.server_port}/", "--browser-console",
        ], env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 50
            result = {}
            while time.monotonic() < deadline:
                if app.poll() is not None:
                    raise RuntimeError(f"App exited {app.returncode}; evidence: {root}")
                try:
                    result = json.loads((root / "page.inspection.json").read_text())
                except (FileNotFoundError, json.JSONDecodeError):
                    pass
                if result.get("stage", 0) >= 9:
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError(f"No inspection result; evidence: {root}")
            time.sleep(0.5)
            result.update(requests=list(hits), owned_pid=app.pid)
            (root / "acceptance.json").write_text(json.dumps(result, indent=2))
            print(json.dumps(result, indent=2))
            assert result["stage"] == 9 and not result["error"], result
            assert not any(p.startswith("/leaked-") for p in hits), hits
            print("PASS: actual CEF selector/form exclusion/native picker/Escape/navigation; no normal page input")
        finally:
            if app.poll() is None:
                app.terminate()
                try:
                    app.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    app.kill()
                    app.wait(timeout=5)
            server.shutdown()
            server.server_close()
            print(f"Evidence: {root}")


if __name__ == "__main__":
    main()
