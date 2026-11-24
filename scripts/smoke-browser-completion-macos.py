#!/usr/bin/env python3
"""Disposable bundled DEBUG app, isolated HOME, two owned loopback fixtures.
Engine/transport acceptance only: no UI click/handoff/provider/physical claim.
Usage: python3 scripts/smoke-browser-completion-macos.py /tmp/Pui.app
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
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    app_path = Path(sys.argv[1]).resolve(strict=True)
    executable = app_path / "Contents/MacOS/pui"
    if not executable.is_file() or not str(app_path).startswith(("/tmp/", "/private/tmp/", "/var/folders/", "/private/var/folders/")):
        raise SystemExit("Use a disposable DEBUG bundle in a temporary directory, never an installed app.")
    root = Path(tempfile.mkdtemp(prefix="pui-completion-acceptance-"))
    hits = []
    lock = threading.Lock()

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            with lock:
                hits.append({"port": self.server.server_port, "path": self.path})
            path = self.path.split("?", 1)[0]
            if path == "/completion":
                body = f'''<!doctype html><title>Pui completion local acceptance</title>
<style>html,body{{margin:0;background:rgb(220,40,60)}}body{{padding:24px}}h1{{margin:0}}#pixel-reference{{position:fixed;top:0;left:0;right:0;height:16px;background:linear-gradient(to right,black 0%,black 50%,white 50%,white 100%);z-index:999}}</style><div id="pixel-reference"></div>
<h1 id="public">Public <strong>nested title</strong><span hidden>HIDDEN_SENTINEL</span><input type="password" value="VALUE_SENTINEL"><span contenteditable>EDITABLE_SENTINEL</span></h1>
<form><input id="password" type="password" value="PASSWORD_SENTINEL"></form>
<iframe src="/frame" style="width:320px;height:160px"></iframe>
<iframe src="http://127.0.0.1:{foreign.server_port}/foreign" style="width:320px;height:160px"></iframe>
<script>
// Poison main-world natives. Production isolated-world reads must not run these.
const poison=()=>{{fetch('/leaked-native-override');throw Error('fixture override');}};
Element.prototype.matches=poison;Element.prototype.getBoundingClientRect=poison;
CharacterData.prototype.substringData=poison;window.getComputedStyle=poison;
</script>'''
            elif path == "/frame":
                body = '''<!doctype html><title>Child fixture</title>
<h2 id="frame-title">Child <b>nested title</b><span hidden>HIDDEN_CHILD_SENTINEL</span></h2>
<section id="shadow-host"></section><script>
const root=document.getElementById('shadow-host').attachShadow({mode:'open'});
root.innerHTML='<h3 id="shadow-title">Shadow <b>nested title</b><form>FORM_SHADOW_SENTINEL</form><span hidden>HIDDEN_SHADOW_SENTINEL</span></h3>';
</script>'''
            elif path == "/foreign":
                body = '<!doctype html><title>Foreign fixture</title><h4>CROSS_ORIGIN_SENTINEL</h4>'
            elif path == "/favicon.ico":
                self.send_response(204)
                self.end_headers()
                return
            else:
                self.send_response(404)
                self.end_headers()
                return
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body.encode())

    foreign = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    for owned in (server, foreign):
        threading.Thread(target=owned.serve_forever, daemon=True).start()
    home = root / "home"
    home.mkdir()
    env = {key: os.environ[key] for key in ("PATH", "TMPDIR", "LANG", "LC_ALL") if key in os.environ}
    env.update(HOME=str(home), CFFIXED_USER_HOME=str(home), XDG_CONFIG_HOME=str(home / "config"), XDG_CACHE_HOME=str(home / "cache"),
               PUI_BROWSER_COMPLETION_SMOKE="1", PUI_BROWSER_SMOKE_REPORT=str(root / "page.json"))
    app = None
    try:
        with (root / "app.log").open("w") as log:
            app = subprocess.Popen([
                str(executable), "--demo", "--cwd", str(root),
                "--open", f"http://127.0.0.1:{server.server_port}/completion",
            ], env=env, stdout=log, stderr=log)
            deadline = time.monotonic() + 50
            result = {}
            while time.monotonic() < deadline:
                if app.poll() is not None:
                    raise RuntimeError(f"Disposable app exited {app.returncode}; evidence: {root}")
                try:
                    result = json.loads((root / "page.inspection.json").read_text())
                except (FileNotFoundError, json.JSONDecodeError):
                    pass
                if result.get("stage", 0) >= 9:
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError(f"No completion result; evidence: {root}")
            with lock:
                requests = list(hits)
            result.update(requests=requests, owned_pid=app.pid, evidence=str(root))
            (root / "acceptance.json").write_text(json.dumps(result, indent=2))
            print(json.dumps(result, indent=2))
            assert result["stage"] == 9 and not result["error"], result
            assert len(result["checks"]) == 6, result
            assert {r["path"] for r in requests} >= {"/completion", "/frame", "/foreign"}, requests
            assert all(r["path"] in {"/completion", "/frame", "/foreign", "/favicon.ico"} for r in requests), requests
            print("PASS: actual isolated scoped reader, cross-origin inventory exclusion, native PNG transport/decode.")
            print("NOT VALIDATED: UI capture/review/handoff, crop/annotation, providers, physical input, sandbox guarantees.")
    finally:
        if app is not None and app.poll() is None:
            app.terminate()
            try:
                app.wait(timeout=15)
            except subprocess.TimeoutExpired:
                app.kill()
                app.wait(timeout=5)
        for owned in (server, foreign):
            owned.shutdown()
            owned.server_close()
        print(f"Evidence retained: {root}")


if __name__ == "__main__":
    main()
