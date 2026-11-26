import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';
import { join } from 'node:path';

const raw = await readFile(new URL('../scripts/shellenv.ts', import.meta.url), 'utf8');
const source = stripTypeScriptTypes(raw.replace(/import\s*\{[^}]*\}\s*from\s*"@earendil-works\/pi-coding-agent";/s, 'const createBashToolDefinition = undefined;'));
const {
  installShellEnv,
  shellProviderEnv,
  sandboxModeFromSettings,
  escapeSeatbeltString,
  buildSeatbeltProfile,
  wrapCommandWithSeatbelt,
} = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);

function fakeBash() {
  const created = [];
  const create = (cwd, options) => {
    const definition = {
      name: 'bash',
      description: 'Run a command',
      cwd,
      execute: async (_id, params) => options.spawnHook({ command: params.command, cwd, env: { PATH: '/bin', OPENAI_API_KEY: params.override } }),
    };
    created.push(definition);
    return definition;
  };
  return { created, create };
}

test('withheld provider keys reach agent shell commands, not Pi', async () => {
  const prevSettings = process.env.PUI_SETTINGS;
  delete process.env.PUI_SETTINGS;
  process.env.PUI_SHELL_PROVIDER_ENV = JSON.stringify({ OPENAI_API_KEY: 'nvapi-secret', GEMINI_API_KEY: 'g' });
  try {
    const tools = [];
    const { created, create } = fakeBash();
    const ctrl = installShellEnv({ registerTool: tool => tools.push(tool) }, create);
    assert.equal(ctrl.installed, true);
    assert.equal(process.env.PUI_SHELL_PROVIDER_ENV, undefined, 'not left in Pi\'s environment');
    assert.equal(tools.length, 1);
    assert.equal(tools[0].name, 'bash');
    const spawned = await tools[0].execute('1', { command: 'npm run dev' }, undefined, undefined, { cwd: '/repo' });
    assert.equal(spawned.command, 'npm run dev');
    assert.equal(spawned.env.OPENAI_API_KEY, 'nvapi-secret');
    assert.equal(spawned.env.GEMINI_API_KEY, 'g');
    assert.equal(spawned.env.PATH, '/bin');
    assert.equal(spawned.cwd, '/repo', 'each chat cwd gets its own tool');
    assert.deepEqual(created.map(d => d.cwd), [process.cwd(), '/repo']);
    const explicit = await tools[0].execute('2', { command: 'x', override: 'explicit' }, undefined, undefined, { cwd: '/repo' });
    assert.equal(explicit.env.OPENAI_API_KEY, 'explicit', 'the command environment wins');
  } finally {
    if (prevSettings === undefined) delete process.env.PUI_SETTINGS; else process.env.PUI_SETTINGS = prevSettings;
  }
});

test('no withheld keys and sandbox off leaves the built-in bash tool untouched', () => {
  const prevSettings = process.env.PUI_SETTINGS;
  delete process.env.PUI_SHELL_PROVIDER_ENV;
  delete process.env.PUI_SETTINGS;
  try {
    const tools = [];
    const ctrl = installShellEnv({ registerTool: tool => tools.push(tool) }, fakeBash().create, { settings: { localSandbox: false } });
    assert.equal(ctrl.installed, false);
    assert.equal(tools.length, 0);
  } finally {
    if (prevSettings === undefined) delete process.env.PUI_SETTINGS; else process.env.PUI_SETTINGS = prevSettings;
  }
});

test('malformed or unsafe payloads are ignored', () => {
  assert.deepEqual(shellProviderEnv('not json'), {});
  assert.deepEqual(shellProviderEnv('["x"]'), {});
  assert.deepEqual(shellProviderEnv(JSON.stringify({ 'bad name': 'x', lower: 'y', OK_KEY: 1, GOOD_KEY: 'v' })), { GOOD_KEY: 'v' });
});

test('sandbox mode and Seatbelt profile generation cover standard, strict, escaping, and non-darwin fallback', () => {
  assert.equal(sandboxModeFromSettings({ localSandbox: false, strictSandbox: true }, 'darwin'), 'off');
  assert.equal(sandboxModeFromSettings({ localSandbox: true, strictSandbox: false }, 'darwin'), 'standard');
  assert.equal(sandboxModeFromSettings({ localSandbox: true, strictSandbox: true }, 'darwin'), 'strict');
  assert.equal(sandboxModeFromSettings({ localSandbox: true, strictSandbox: true }, 'linux'), 'off');
  assert.equal(sandboxModeFromSettings({ localSandbox: true, strictSandbox: true }, 'win32'), 'off');

  assert.equal(escapeSeatbeltString('/tmp/"a"\\b\nc'), '/tmp/\\"a\\"\\\\b\\nc');

  assert.equal(buildSeatbeltProfile('/workspace', 'off'), null);
  const std = buildSeatbeltProfile('/workspace/my "repo"', 'standard', { tmpdir: '/private/var/folders/x/T', home: '/Users/dev' });
  assert.match(std, /\(version 1\)/);
  assert.match(std, /\(deny file-write\*\)/);
  assert.ok(std.includes('(subpath "/workspace/my \\"repo\\"")'));
  assert.ok(std.includes('(subpath "/tmp")'));
  assert.ok(std.includes('(subpath "/private/tmp")'));
  assert.ok(std.includes('(literal "/dev/null")'));
  assert.ok(std.includes('(subpath "/dev/fd")'));
  assert.ok(std.includes('(subpath "/Users/dev/.ssh")'));
  assert.ok(std.includes('(literal "/Users/dev/.netrc")'));
  assert.ok(!std.includes('(deny network-outbound)'));

  const strict = buildSeatbeltProfile('/workspace', 'strict', { tmpdir: '/private/tmp', home: '/Users/dev' });
  assert.ok(strict.includes('(deny network-outbound)'));
  assert.ok(strict.includes('(allow network-outbound (remote ip "localhost:*"))'));
  assert.ok(strict.includes('(allow network* (remote unix-socket))'));
  assert.ok(!strict.includes('(local ip "localhost:*")'));
});

test('installShellEnv wraps bash on darwin when localSandbox is enabled and reacts to live update()', async () => {
  delete process.env.PUI_SHELL_PROVIDER_ENV;
  const tools = [];
  const { create } = fakeBash();

  // Starts off; enabling via update() registers and wraps commands.
  const ctrl = installShellEnv({ registerTool: tool => tools.push(tool) }, create, {
    settings: { localSandbox: false, strictSandbox: false },
    platform: 'darwin',
  });
  assert.equal(ctrl.installed, false);
  assert.equal(tools.length, 0);

  ctrl.update({ localSandbox: true, strictSandbox: false });
  assert.equal(ctrl.installed, true);
  assert.equal(tools.length, 1);

  const stdSpawn = await tools[0].execute('1', { command: "echo 'hi'" }, undefined, undefined, { cwd: '/tmp' });
  assert.match(stdSpawn.command, /^exec \/usr\/bin\/sandbox-exec -p '/);
  assert.ok(!stdSpawn.command.includes('(deny network-outbound)'));

  // Upgrade to strict mode live without re-registering.
  ctrl.update({ localSandbox: true, strictSandbox: true });
  assert.equal(tools.length, 1);
  const strictSpawn = await tools[0].execute('2', { command: 'curl https://example.com' }, undefined, undefined, { cwd: '/tmp' });
  assert.ok(strictSpawn.command.includes('(deny network-outbound)'));

  // Disable sandbox live -> passes command through unmodified.
  ctrl.update({ localSandbox: false, strictSandbox: false });
  const offSpawn = await tools[0].execute('3', { command: 'echo plain' }, undefined, undefined, { cwd: '/tmp' });
  assert.equal(offSpawn.command, 'echo plain');

  // Non-darwin stays untouched even with sandbox enabled.
  const linuxTools = [];
  const linuxCtrl = installShellEnv({ registerTool: tool => linuxTools.push(tool) }, fakeBash().create, {
    settings: { localSandbox: true, strictSandbox: true },
    platform: 'linux',
  });
  assert.equal(linuxCtrl.installed, false);
  assert.equal(linuxTools.length, 0);
});

test('wrapped Seatbelt command enforces workspace boundary on macOS', { skip: process.platform !== 'darwin' }, () => {
  const root = mkdtempSync('/var/tmp/pui-shellenv-sb-');
  const workspace = join(root, 'workspace');
  const sibling = join(root, 'sibling');
  mkdirSync(workspace);
  mkdirSync(sibling);
  try {
    const okFile = join(workspace, 'ok.txt');
    const okCmd = wrapCommandWithSeatbelt(`printf 'ok' > '${okFile}'`, workspace, 'standard');
    execFileSync('/bin/bash', ['-c', okCmd], { cwd: workspace });
    assert.equal(readFileSync(okFile, 'utf8'), 'ok');

    const deniedFile = join(sibling, 'nope.txt');
    const denyCmd = wrapCommandWithSeatbelt(`printf 'nope' > '${deniedFile}'`, workspace, 'standard');
    assert.throws(() => execFileSync('/bin/bash', ['-c', denyCmd], { cwd: workspace, stdio: 'pipe' }));
    assert.equal(existsSync(deniedFile), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
