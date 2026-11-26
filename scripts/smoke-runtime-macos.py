#!/usr/bin/env python3
"""Offline, fresh-HOME smoke of a relocated app's bundled agent and resources.

No model prompts, provider keys, global Pi/Node, shell profiles or auth copying.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import selectors
import shutil
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
ADAPTER_FILES = ("ultracode.ts", "permissions.ts", "compaction.ts", "context.ts", "titles.ts", "rewind.ts", "questions.ts", "open.ts", "visual.ts", "turn_ux.ts", "workflows.ts", "antigravity.ts", "accounts.ts", "model-fallback.ts", "shellenv.ts")


def tree_digest(root):
    digest = hashlib.sha256()
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            digest.update(path.relative_to(root).as_posix().encode() + os.readlink(path).encode())
        elif path.is_file():
            digest.update(path.relative_to(root).as_posix().encode())
            with path.open("rb") as source:
                for chunk in iter(lambda: source.read(1024 * 1024), b""):
                    digest.update(chunk)
    return digest.hexdigest()


def rpc_smoke(runtime, env, cwd, adapter=None):
    command = [str(runtime / "bin/pi"), "--mode", "rpc", "--no-session", "--no-extensions", "--no-skills", "--no-prompt-templates"]
    if adapter:
        command += ["--extension", str(adapter)]
    child = subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    selector = selectors.DefaultSelector()
    selector.register(child.stdout, selectors.EVENT_READ)
    buffer = b""
    ready = not adapter
    try:
        child.stdin.write(b'{"id":"runtime-smoke","type":"get_state"}\n')
        child.stdin.flush()
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if not selector.select(timeout=0.2):
                if child.poll() is not None:
                    raise ValueError("Bundled Pi RPC exited before replying.")
                continue
            chunk = os.read(child.stdout.fileno(), 65536)
            if not chunk:
                raise ValueError("Bundled Pi RPC closed before replying.")
            buffer += chunk
            while b"\n" in buffer:
                line, buffer = buffer.split(b"\n", 1)
                record = json.loads(line)
                if record.get("type") == "extension_ui_request" and record.get("statusKey") == "pui-ultracode":
                    ready = True
                if record.get("id") == "runtime-smoke":
                    if not record.get("success"):
                        raise ValueError("Bundled Pi RPC rejected get_state.")
                    if not ready:
                        raise ValueError("Pui adapter did not announce readiness.")
                    return
        raise ValueError("Bundled Pi RPC startup timed out.")
    finally:
        selector.close()
        child.stdin.close()
        try:
            child.wait(timeout=4)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()


def smoke(app):
    required = [Path("Contents/MacOS/pui"), *(Path("Contents/Resources") / name for name in ADAPTER_FILES)]
    for file in required:
        if not (app / file).is_file():
            raise ValueError(f"Packaged Pui file is missing: {file}")
    with tempfile.TemporaryDirectory(prefix="pui-clean-runtime-") as temp:
        root = Path(temp)
        relocated = root / "Relocated Pui.app"
        # Copy only runtime/resources: no dependency on the original path and no
        # developer agent directory. Full .app/UI smoke is a separate check.
        resources = relocated / "Contents/Resources"
        shutil.copytree(app / "Contents/Resources/runtime", resources / "runtime", symlinks=True)
        runtime = resources / "runtime"
        home = root / "fresh-home"
        cwd = root / "empty-workspace"
        home.mkdir()
        cwd.mkdir()
        env = {"PATH": str(runtime / "bin") + ":/usr/bin:/bin", "HOME": str(home), "TMPDIR": temp,
               "PI_OFFLINE": "1", "PI_SKIP_VERSION_CHECK": "1", "JITI_FS_CACHE": "false",
               "NODE_COMPILE_CACHE": str(root / "node-cache"), "npm_config_cache": str(root / "npm-cache"),
               "npm_config_prefix": str(home / ".pi/agent/npm-global")}
        executable = app / "Contents/MacOS/pui"
        (relocated / "Contents/MacOS").mkdir()
        shutil.copy2(executable, relocated / "Contents/MacOS/pui")
        clean = {**env, "PATH": "/usr/bin:/bin"}
        info = json.loads(subprocess.check_output([str(relocated / "Contents/MacOS/pui"), "--runtime-info"],
                                                  env=clean, cwd=cwd, text=True, timeout=20))
        if not all(str(relocated) in str(info[key]) for key in ("bundled", "pi", "node", "sdk")):
            raise ValueError("Pui did not resolve its relocated bundled runtime.")
        before = tree_digest(runtime)
        subprocess.run([str(relocated / "Contents/MacOS/pui"), "--runtime-check"], env=clean, cwd=cwd, check=True, timeout=25)
        manifest = json.loads((runtime / "manifest.json").read_text())
        for binary, args, expected in (("node", ["--version"], "v" + manifest["node"]),
                                        ("pi", ["--version"], manifest["pi"])):
            output = subprocess.check_output([str(runtime / "bin" / binary), *args], env=env, cwd=cwd, text=True, timeout=20).strip()
            if output != expected:
                raise ValueError("Bundled runtime version mismatch.")
        for binary in ("npm", "npx", "rg", "fd"):
            subprocess.run([str(runtime / "bin" / binary), "--version"], env=env, cwd=cwd, check=True, stdout=subprocess.DEVNULL, timeout=20)
        sdk = runtime / "pi/node_modules/@earendil-works/pi-coding-agent/dist/index.js"
        # SDK-internal imports and QuickJS/Photon WASM remain part of distribution.
        script = """
import {pathToFileURL} from 'node:url';
const sdk=await import(pathToFileURL(process.argv[1]).href);
if(typeof sdk.SessionManager!=='function'||typeof sdk.createAgentSession!=='function')throw Error('SDK missing');
const sm=sdk.SessionManager.inMemory(); if(!sm.getSessionId())throw Error('session fixture failed');
const fs=await import('node:fs/promises');
await fs.writeFile('runtime-fixture.txt','bundled-search-marker');
const grep=await sdk.createGrepTool(process.cwd()).execute('smoke-grep',{pattern:'bundled-search-marker',path:'.'});
if(!JSON.stringify(grep).includes('runtime-fixture.txt'))throw Error('bundled grep failed');
const found=await sdk.createFindTool(process.cwd()).execute('smoke-find',{pattern:'*.txt',path:'.'});
if(!JSON.stringify(found).includes('runtime-fixture.txt'))throw Error('bundled fd failed');
const bash=await sdk.createBashTool(process.cwd()).execute('smoke-bash',{command:'printf bundled-bash-marker'});
if(!JSON.stringify(bash).includes('bundled-bash-marker'))throw Error('system shell failed');
const config=await import(new URL('./config.js',pathToFileURL(process.argv[1])).href);
const {createRequire}=await import('node:module');const require=createRequire(pathToFileURL(process.argv[1]));
const {QuickJS}=require('quickjs-wasi');
const vm=await QuickJS.create({wasm:await fs.readFile(config.getQuickJSWasmPath())});
const result=vm.evalCode('1+2');if(result.toNumber()!==3)throw Error('WASM execution failed');result.dispose();vm.dispose();
const native=await import(new URL('../../pi-tui/dist/native-platform.js',pathToFileURL(process.argv[1])).href);
if(!native.getNativePlatformHelper())throw Error('native arm64 addon unavailable');
"""
        subprocess.run([str(runtime / "bin/node"), "--input-type=module", "--eval", script, str(sdk)], env=env, cwd=cwd, check=True, timeout=25)
        for name in ADAPTER_FILES:
            shutil.copy2(app / "Contents/Resources" / name, resources / name)
        env["PUI_PI_SDK"] = str(sdk)
        rpc_smoke(runtime, env, cwd, resources / "ultracode.ts")
        # Real Settings resource helper, no provider/MCP/extension execution.
        env["PUI_RESOURCES_HELPER"] = "1"
        output = subprocess.check_output([str(runtime / "bin/node"), "--input-type=module", "--eval",
                                          (ROOT / "scripts/resources.mjs").read_text(), str(sdk)],
                                         input=json.dumps({"kind": "skills", "action": "list", "cwd": str(cwd)}),
                                         env=env, cwd=cwd, text=True, timeout=30)
        response = json.loads(output)
        if "error" in response or "data" not in response:
            raise ValueError("Bundled resources helper failed.")
        if before != tree_digest(runtime):
            raise ValueError("Smoke test changed the immutable runtime payload.")
        print(f"Offline relocated runtime smoke passed: Pi {manifest['pi']}, Node {manifest['node']}, RPC, SDK and resources.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", type=Path)
    args = parser.parse_args()
    smoke(args.app.resolve())
