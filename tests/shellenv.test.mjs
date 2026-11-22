import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';

const raw = await readFile(new URL('../scripts/shellenv.ts', import.meta.url), 'utf8');
const source = stripTypeScriptTypes(raw.replace(/import\s*\{[^}]*\}\s*from\s*"@earendil-works\/pi-coding-agent";/s, 'const createBashToolDefinition = undefined;'));
const { installShellEnv, shellProviderEnv } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);

function fakeBash() {
  const created = [];
  const create = (cwd, options) => {
    const definition = { name: 'bash', description: 'Run a command', cwd, execute: async (_id, params) => options.spawnHook({ command: params.command, cwd, env: { PATH: '/bin', OPENAI_API_KEY: params.override } }) };
    created.push(definition);
    return definition;
  };
  return { created, create };
}

test('withheld provider keys reach agent shell commands, not Pi', async () => {
  process.env.PUI_SHELL_PROVIDER_ENV = JSON.stringify({ OPENAI_API_KEY: 'nvapi-secret', GEMINI_API_KEY: 'g' });
  const tools = [];
  const { created, create } = fakeBash();
  assert.equal(installShellEnv({ registerTool: tool => tools.push(tool) }, create), true);
  assert.equal(process.env.PUI_SHELL_PROVIDER_ENV, undefined, 'not left in Pi\'s environment');
  assert.equal(tools.length, 1);
  assert.equal(tools[0].name, 'bash');
  const spawned = await tools[0].execute('1', { command: 'npm run dev' }, undefined, undefined, { cwd: '/repo' });
  assert.equal(spawned.env.OPENAI_API_KEY, 'nvapi-secret');
  assert.equal(spawned.env.GEMINI_API_KEY, 'g');
  assert.equal(spawned.env.PATH, '/bin');
  assert.equal(spawned.cwd, '/repo', 'each chat cwd gets its own tool');
  assert.deepEqual(created.map(d => d.cwd), [process.cwd(), '/repo']);
  const explicit = await tools[0].execute('2', { command: 'x', override: 'explicit' }, undefined, undefined, { cwd: '/repo' });
  assert.equal(explicit.env.OPENAI_API_KEY, 'explicit', 'the command environment wins');
});

test('no withheld keys leaves the built-in bash tool untouched', () => {
  delete process.env.PUI_SHELL_PROVIDER_ENV;
  const tools = [];
  assert.equal(installShellEnv({ registerTool: tool => tools.push(tool) }, fakeBash().create), false);
  assert.equal(tools.length, 0);
});

test('malformed or unsafe payloads are ignored', () => {
  assert.deepEqual(shellProviderEnv('not json'), {});
  assert.deepEqual(shellProviderEnv('["x"]'), {});
  assert.deepEqual(shellProviderEnv(JSON.stringify({ 'bad name': 'x', lower: 'y', OK_KEY: 1, GOOD_KEY: 'v' })), { GOOD_KEY: 'v' });
});
