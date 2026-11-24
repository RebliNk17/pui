#!/usr/bin/env python3
"""Actual CEF ephemeral-context privacy acceptance against an owned loopback fixture.

Usage: python3 scripts/smoke-browser-privacy-macos.py /tmp/DisposablePui.app
Only a disposable DEBUG bundle outside /Applications is allowed. The script
launches and terminates only its own child, with isolated HOME and no providers.
Evidence is retained. No installed app, profile, Keychain or external site used.
"""
import argparse
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time
from urllib.parse import parse_qs, urlsplit

FIXTURE = b'''<!doctype html><meta charset="utf-8"><link rel="icon" href="data:,">
<title>Pui privacy fixture</title><h1>Local ephemeral storage acceptance</h1>
<script>
window.privacyFixture = async function(operation, tag) {
  const key = 'pui-privacy-' + tag;
  const db = await new Promise((resolve, reject) => {
    const request = indexedDB.open('pui-privacy-fixture', 1);
    request.onupgradeneeded = () => request.result.createObjectStore('markers');
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(Error('IDB open failed'));
  });
  try {
    if (operation === 'seed') {
      await fetch('/set?tag=' + encodeURIComponent(tag), {cache:'no-store'});
      localStorage.setItem(key, 'present');
      await new Promise((resolve, reject) => {
        const tx = db.transaction('markers', 'readwrite');
        tx.objectStore('markers').put('present', key);
        tx.oncomplete = resolve; tx.onerror = () => reject(Error('IDB write failed'));
      });
      const cache = await caches.open('pui-privacy-fixture');
      await cache.put('/cached-' + tag, new Response('present'));
    }
    const cookie = await (await fetch('/echo?tag=' + encodeURIComponent(tag), {cache:'no-store'})).json();
    const indexed = await new Promise((resolve, reject) => {
      const tx = db.transaction('markers', 'readonly');
      const request = tx.objectStore('markers').get(key);
      request.onsuccess = () => resolve(request.result === 'present');
      request.onerror = () => reject(Error('IDB read failed'));
    });
    const cache = await caches.open('pui-privacy-fixture');
    return {
      http_only: cookie.present,
      http_only_not_script_visible: !document.cookie.includes('pui_probe_' + tag + '='),
      local_storage: localStorage.getItem(key) === 'present',
      indexed_db: indexed,
      cache_storage: Boolean(await cache.match('/cached-' + tag)),
    };
  } finally { db.close(); }
};
</script>'''


def read_report(path):
    try:
        return json.loads(path.read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return {}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('bundle', type=Path)
    args = parser.parse_args()
    bundle = args.bundle.resolve(strict=True)
    if bundle.suffix != '.app' or any(part == 'Applications' for part in bundle.parts):
        parser.error('Use only a disposable DEBUG .app bundle outside every Applications directory')
    executable = bundle / 'Contents/MacOS/pui'
    if not executable.is_file():
        parser.error('Bundle has no Contents/MacOS/pui executable')
    root = Path(tempfile.mkdtemp(prefix='pui-privacy-acceptance-'))
    report = root / 'privacy.json'
    records = []
    lock = threading.Lock()

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            parsed = urlsplit(self.path)
            tag = parse_qs(parsed.query).get('tag', [''])[0]
            if parsed.path not in {'/a', '/b', '/default', '/other', '/set', '/echo'} or tag not in {'', 'work', 'other'}:
                self.send_error(404)
                return
            # Only booleans are retained; never log Cookie/header/storage values.
            cookie_present = f'pui_probe_{tag}=fixture' in self.headers.get('Cookie', '')
            with lock:
                records.append({'path': parsed.path, 'tag': tag, 'cookie_present': cookie_present})
            body = json.dumps({'present': cookie_present}).encode() if parsed.path in {'/set', '/echo'} else FIXTURE
            self.send_response(200)
            self.send_header('Content-Type', 'application/json' if parsed.path in {'/set', '/echo'} else 'text/html; charset=utf-8')
            self.send_header('Cache-Control', 'no-store')
            self.send_header('Content-Length', str(len(body)))
            if parsed.path == '/set':
                self.send_header('Set-Cookie', f'pui_probe_{tag}=fixture; HttpOnly; SameSite=Strict; Path=/')
            self.end_headers()
            try:
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError):
                pass

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    home = root / 'home'
    home.mkdir(mode=0o700)
    # Deliberately do not inherit provider credentials, proxy settings or PUI flags.
    env = {key: os.environ[key] for key in ['PATH', 'TMPDIR', 'LANG', 'LC_ALL'] if key in os.environ}
    env.update(HOME=str(home), CFFIXED_USER_HOME=str(home), XDG_CONFIG_HOME=str(home / 'config'),
               XDG_CACHE_HOME=str(home / 'cache'), PUI_BROWSER_PRIVACY_SMOKE_REPORT=str(report))
    app = None
    quiet_hits = None
    result = {}
    with (root / 'app.log').open('w') as log:
        try:
            app = subprocess.Popen([
                str(executable), '--demo', '--cwd', str(root),
                '--open', f'http://127.0.0.1:{server.server_port}/a',
            ], env=env, stdout=log, stderr=log)
            deadline = time.monotonic() + 95
            while time.monotonic() < deadline:
                if app.poll() is not None:
                    raise RuntimeError(f'Owned app exited {app.returncode}')
                result = read_report(report)
                if result.get('stage') == 'failed':
                    raise RuntimeError(result.get('error', 'Privacy probe failed'))
                if result.get('stage') == 'paused' and quiet_hits is None:
                    with lock:
                        before = len(records)
                    # An event-free pause proves restart itself does not reload.
                    time.sleep(1.5)
                    with lock:
                        after = len(records)
                    assert before == after, 'Requests occurred while Work tabs were suspended'
                    quiet_hits = after
                    report.with_suffix('.resume').write_text('Explicit local acceptance Resume\n')
                if result.get('stage') == 'passed':
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError('No bounded privacy result (DEBUG probe hook/bundle required)')
            assert quiet_hits is not None, 'Probe skipped explicit no-network Resume handshake'
            checks = result.get('checks', {})
            assert checks and all(value is True for value in checks.values()), checks
            required = {'work_shared_http_only', 'work_shared_local_storage', 'work_shared_indexed_db',
                        'work_shared_cache_storage', 'default_isolated_http_only', 'other_isolated_http_only',
                        'restarted_work_empty_http_only', 'restarted_work_empty_indexed_db',
                        'restarted_work_empty_cache_storage', 'other_retained_http_only',
                        'other_retained_local_storage', 'work_same_native_context', 'work_storage_sharing',
                        'default_native_isolated', 'other_native_isolated', 'old_work_browsers_closed',
                        'unresumed_work_peer_still_stopped', 'all_probe_browsers_closed'}
            assert required <= checks.keys(), 'Missing required CEF privacy assertions'
            with lock:
                final_records = list(records)
            assert [r['path'] for r in final_records if r['path'] in {'/a', '/b', '/default', '/other'}] == ['/a', '/b', '/default', '/other', '/a'], 'Unexpected automatic tab reload/navigation'
            result.update(owned_pid=app.pid, no_requests_while_suspended=True, request_count=len(final_records), requests=final_records)
            (root / 'acceptance.json').write_text(json.dumps(result, indent=2))
            print(json.dumps(result, indent=2))
            print('PASS: actual CEF shared/isolated cookies + localStorage/IndexedDB/CacheStorage; group-only restart, explicit Resume, teardown')
        finally:
            if app is not None and app.poll() is None:
                app.terminate()
                try:
                    app.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    app.kill()  # Only our exact disposable child, never other Pui builds.
                    app.wait(timeout=5)
            server.shutdown()
            server.server_close()
            print(f'Evidence retained: {root}')


if __name__ == '__main__':
    main()
