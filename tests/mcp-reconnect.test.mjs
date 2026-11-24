// Real built-in MCP extension, fake transports only: no user config or provider traffic.
import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

let sdkPath = process.env.PUI_PI_SDK;
if (!sdkPath) {
  const agent = process.env.PI_CODING_AGENT_DIR ?? join(homedir(), '.pi/agent');
  try {
    const version = readFileSync(join(agent, 'install/current-version'), 'utf8').trim();
    sdkPath = join(agent, 'install/releases', version, 'node_modules/@earendil-works/pi-coding-agent/dist/index.js');
  } catch {}
}
const available = Boolean(sdkPath && existsSync(sdkPath));
let createMcpExtension, createInMemoryTransportPair;
if (available) {
  ({ createMcpExtension } = await import(pathToFileURL(sdkPath).href));
  // The public ./testing export is import-only; createRequire.resolve cannot resolve it.
  ({ createInMemoryTransportPair } = await import(new URL('../../pi-mcp/dist/testing/index.js', pathToFileURL(sdkPath)).href));
}
function fixture(t) {
  const handlers = new Map(), commands = new Map(), tools = new Map();
  const notices = [], wireErrors = [], opened = { alpha: [], beta: [] }, fail = new Set();
  const calls = Promise.withResolvers();
  let active = [];
  const pi = {
    on: (name, handler) => handlers.set(name, handler),
    registerCommand: (name, command) => commands.set(name, command),
    registerToolRenderer() {},
    registerTool: tool => tools.set(tool.name, tool),
    getMcpServers: () => [], getAllTools: () => [...tools.values()],
    getActiveTools: () => active, setActiveTools: names => { active = names; },
  };
  const ctx = {
    cwd: tmpdir(), mode: 'rpc', hasUI: true, isIdle: () => false,
    signal: new AbortController().signal,
    ui: {
      notify: (message, level) => notices.push({ message, level }),
      select: async () => { throw new Error('Explicit server must not open a picker'); },
    },
  };
  createMcpExtension({
    loadConfig: () => ({ errors: [], servers: ['alpha', 'beta'].map(name => ({
      name, source: 'fixture', scope: 'extension',
      config: { command: 'never-executed', exposure: 'direct', timeout: 2 },
    })) }),
    credentials: {
      forServer() { throw new Error('Unexpected credential access'); },
      tokens() { throw new Error('Unexpected credential access'); },
      remove() { throw new Error('Unexpected credential access'); },
    },
    updateConfig() { throw new Error('Unexpected config write'); },
    logPath: join(tmpdir(), 'pui-mcp-unused-fixture.log'),
    createTransport(entry) {
      const name = entry.name;
      if (fail.has(name)) throw new Error('fixture reconnect failure');
      const { client, server } = createInMemoryTransportPair();
      const connection = { closed: false };
      opened[name].push(connection);
      client.onClose(() => { connection.closed = true; });
      void server.start();
      server.onMessage(message => {
        if (message.id === undefined) return;
        let result;
        if (message.method === 'initialize') {
          result = { protocolVersion: message.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: `fixture-${name}`, version: '1' } };
        } else if (message.method === 'tools/list') {
          result = { tools: [{ name: 'hold', description: 'Wait until transport closes', inputSchema: { type: 'object', properties: {} } }] };
        } else if (message.method === 'tools/call') {
          calls.resolve({ name, message });
          return; // Leave request in flight so reconnect must reject it.
        } else {
          wireErrors.push(new Error(`Unexpected request: ${message.method}`));
          return;
        }
        void server.send({ jsonrpc: '2.0', id: message.id, result }).catch(error => wireErrors.push(error));
      });
      return client;
    },
  })(pi);
  const command = args => commands.get('mcp').handler(args, ctx);
  t.after(async () => { await handlers.get('session_shutdown')({}, ctx); assert.deepEqual(wireErrors, []); });
  return {
    tools, notices, opened, fail, calls, command,
    async start() {
      await handlers.get('session_start')({}, ctx);
      await command('');
      assert.equal(opened.alpha.length, 1);
      assert.equal(opened.beta.length, 1);
      notices.length = 0;
    },
  };
}
const options = { skip: available ? false : 'Installed Pi SDK unavailable', timeout: 10_000 };
test('named MCP reconnect works during a busy context and replaces only its transport', options, async t => {
  const h = fixture(t); await h.start();
  await h.command('reconnect alpha');
  assert.equal(h.opened.alpha.length, 2);
  assert.equal(h.opened.alpha[0].closed, true);
  assert.equal(h.opened.alpha[1].closed, false);
  assert.equal(h.opened.beta.length, 1);
  assert.equal(h.opened.beta[0].closed, false);
  assert.ok(h.notices.some(n => n.level === 'info' && n.message.startsWith('Reconnected to MCP server "alpha" (')));
  assert.ok(!h.notices.some(n => n.level === 'error'));
});
test('MCP reconnect failure is notified separately rather than thrown by command handler', options, async t => {
  const h = fixture(t); await h.start(); h.fail.add('alpha');
  await assert.doesNotReject(() => h.command('reconnect alpha'));
  assert.ok(h.notices.some(n => n.level === 'error' && n.message.includes('fixture reconnect failure')));
  assert.ok(!h.notices.some(n => n.message.startsWith('Reconnected to MCP server "alpha"')));
  assert.equal(h.opened.beta.length, 1);
  assert.equal(h.opened.beta[0].closed, false);
});
test('MCP reconnect rejects already in-flight tool requests rather than replaying them', options, async t => {
  const h = fixture(t); await h.start();
  const tool = h.tools.get('mcp__alpha__hold');
  assert.ok(tool);
  const result = Promise.resolve().then(() => tool.execute('held-call', {}, undefined, undefined, {}));
  // Attach before reconnect to avoid an unhandled-rejection race.
  const settled = result.then(value => ({ value }), error => ({ error }));
  assert.equal((await h.calls.promise).name, 'alpha');
  await h.command('reconnect alpha');
  const outcome = await settled;
  assert.ok(outcome.error || outcome.value?.isError, 'The old connection cannot complete its request');
  assert.equal(h.opened.alpha.length, 2);
  assert.equal(h.opened.beta.length, 1);
});
