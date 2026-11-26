import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { stripTypeScriptTypes } from 'node:module';

const browserRaw = await readFile(new URL('../scripts/browser.ts', import.meta.url), 'utf8');
const browserSource = stripTypeScriptTypes(
  browserRaw.replace(
    'import { Type } from "typebox";',
    'const Type = new Proxy({}, { get: (_, k) => (...args) => ({ type: String(k), args }) });',
  ),
);
const { installBrowser, BROWSER_TOOLS, BROWSER_GUIDANCE } = await import(
  `data:text/javascript;base64,${Buffer.from(browserSource).toString('base64')}`
);

const permsRaw = await readFile(new URL('../scripts/permissions.ts', import.meta.url), 'utf8');
const permsSource = stripTypeScriptTypes(permsRaw);
const { installPermissions, isLocalBrowserUrl } = await import(
  `data:text/javascript;base64,${Buffer.from(permsSource).toString('base64')}`
);

function browserHost({ reply, readonly = false, taskRole = '' } = {}) {
  const tools = new Map();
  const requests = [];
  installBrowser({ registerTool: (t) => tools.set(t.name, t) }, { readonly, taskRole });
  const ctx = {
    hasUI: true,
    mode: 'rpc',
    cwd: '/workspace',
    ui: {
      input: async (...args) => {
        requests.push(args);
        if (reply) return reply(...args);
        return JSON.stringify({ ok: true });
      },
    },
  };
  return {
    tools,
    requests,
    ctx,
    run: (name, args = {}, signal) => tools.get(name).execute('call', args, signal, undefined, ctx),
  };
}

const model = (id, provider, input, output) => ({
  id,
  provider,
  cost: { input, output },
  input: ['text'],
  contextWindow: 32000,
  maxTokens: 2048,
});

const cwd = await mkdtemp(join(tmpdir(), 'pui-browser-perms-'));

function permHost(mode, { decision = 'allow', approve = false, readonly = false } = {}) {
  const events = new Map();
  const prompts = [];
  const calls = [];
  const pi = { on: (n, f) => events.set(n, f), appendEntry: () => {} };
  const ctx = {
    cwd,
    hasUI: true,
    model: model('main', 'current', 5, 10),
    modelRegistry: {
      getAvailable: () => [model('cheap', 'current', 0.1, 0.5)],
      streamSimple: (m, c, o) => {
        calls.push({ m, c, o });
        return {
          result: async () => ({
            content: [{ type: 'text', text: JSON.stringify({ decision, reason: 'ok' }) }],
            stopReason: 'stop',
            usage: { cost: { total: 0.001 } },
          }),
        };
      },
    },
    ui: {
      setStatus: () => {},
      confirm: async (...a) => {
        prompts.push(a);
        return approve;
      },
    },
  };
  installPermissions(pi, { permission: mode, readonly });
  events.get('before_agent_start')({
    messageType: 'user',
    prompt: 'Verify the local web app in the browser',
    systemPrompt: 'base',
  });
  return {
    prompts,
    calls,
    gate: (toolName, input = {}) => events.get('tool_call')({ toolName, input }, ctx),
  };
}

test('registers all 5 sequential browser tools with crisp schemas and guidance', () => {
  const h = browserHost();
  assert.deepEqual([...h.tools.keys()], [
    'pui_browser_navigate',
    'pui_browser_read',
    'pui_browser_act',
    'pui_browser_screenshot',
    'pui_browser_console',
  ]);
  assert.deepEqual([...BROWSER_TOOLS], [...h.tools.keys()]);
  for (const tool of h.tools.values()) {
    assert.equal(tool.executionMode, 'sequential');
    assert.equal(tool.parameters.type, 'Object');
    assert.ok(tool.description.length > 20);
  }
  assert.match(BROWSER_GUIDANCE, /pui_browser_read/);
  assert.match(BROWSER_GUIDANCE, /pui_browser_screenshot/);
  assert.match(BROWSER_GUIDANCE, /take over/);
});

test('forwards all 5 tools through Pui agent control with expected actions and payloads', async () => {
  const h = browserHost({
    reply: async (_title, raw) => {
      const req = JSON.parse(raw);
      if (req.action === 'browser_navigate') {
        return JSON.stringify({ url: req.url, title: 'App', loading: true, opened: true });
      }
      if (req.action === 'browser_read') {
        return JSON.stringify({ url: 'http://localhost:3000/', title: 'App', text: 'Hello', elements: [] });
      }
      if (req.action === 'browser_act') {
        return JSON.stringify({ ok: true, kind: req.kind });
      }
      if (req.action === 'browser_screenshot') {
        return JSON.stringify({
          path: '/private/tmp/pui-browser-shot-1.png',
          bytes: 123,
          width: 800,
          height: 600,
          full_page: Boolean(req.full_page),
          url: 'http://localhost:3000/',
          title: 'App',
          base64: 'iVBORw0KGgo=',
        });
      }
      if (req.action === 'browser_console') {
        return JSON.stringify({ url: 'http://localhost:3000/', total: 1, errors: 1, warnings: 0, returned: 1, entries: [] });
      }
      return JSON.stringify({ error: 'unexpected' });
    },
  });

  const nav = await h.run('pui_browser_navigate', { url: 'http://localhost:3000' });
  assert.equal(h.requests[0][0], 'Pui agent control');
  const navPayload = JSON.parse(h.requests[0][1]);
  assert.equal(navPayload.version, 1);
  assert.equal(navPayload.action, 'browser_navigate');
  assert.equal(navPayload.url, 'http://localhost:3000/');
  assert.equal(nav.details.pui.opened, true);

  await h.run('pui_browser_read', { max_chars: 4000, include_elements: false });
  assert.deepEqual(
    (({ action, max_chars, include_elements }) => ({ action, max_chars, include_elements }))(
      JSON.parse(h.requests[1][1]),
    ),
    { action: 'browser_read', max_chars: 4000, include_elements: false },
  );

  await h.run('pui_browser_act', { kind: 'type', selector: '#email', text: 'a@b.com', clear: true });
  assert.deepEqual(
    (({ action, kind, selector, text, clear }) => ({ action, kind, selector, text, clear }))(
      JSON.parse(h.requests[2][1]),
    ),
    { action: 'browser_act', kind: 'type', selector: '#email', text: 'a@b.com', clear: true },
  );

  const shot = await h.run('pui_browser_screenshot', { full_page: true });
  assert.equal(JSON.parse(h.requests[3][1]).action, 'browser_screenshot');
  assert.equal(JSON.parse(h.requests[3][1]).full_page, true);
  // Base64 is attached as an image block and NOT dumped into JSON text content.
  assert.equal(shot.content.length, 2);
  assert.equal(shot.content[0].type, 'text');
  assert.doesNotMatch(shot.content[0].text, /iVBORw0KGgo=/);
  assert.equal(shot.content[1].type, 'image');
  assert.equal(shot.content[1].mimeType, 'image/png');
  assert.equal(shot.content[1].data, 'iVBORw0KGgo=');
  assert.equal(shot.details.pui.path, '/private/tmp/pui-browser-shot-1.png');

  await h.run('pui_browser_console', { limit: 25, level: 'error' });
  assert.deepEqual(
    (({ action, limit, level }) => ({ action, limit, level }))(JSON.parse(h.requests[4][1])),
    { action: 'browser_console', limit: 25, level: 'error' },
  );
});

test('client-side parameter validation rejects unsafe URLs, invalid actions, and out-of-range params', async () => {
  const h = browserHost();
  for (const url of [
    '',
    'javascript:alert(1)',
    'file:///etc/passwd',
    'data:text/html,hi',
    'https://user:pass@example.com',
    'http://localhost:3000/\nfoo',
  ]) {
    await assert.rejects(h.run('pui_browser_navigate', { url }));
  }
  await assert.rejects(h.run('pui_browser_read', { max_chars: 50 }));
  await assert.rejects(h.run('pui_browser_read', { max_chars: 40000 }));
  await assert.rejects(h.run('pui_browser_act', { kind: 'click' }));
  await assert.rejects(h.run('pui_browser_act', { kind: 'type', selector: '#x' }));
  await assert.rejects(h.run('pui_browser_act', { kind: 'press', key: 'F12' }));
  await assert.rejects(h.run('pui_browser_act', { kind: 'wait', ms: 10000 }));
  await assert.rejects(h.run('pui_browser_act', { kind: 'eval' }));
  await assert.rejects(h.run('pui_browser_console', { limit: 0 }));
  await assert.rejects(h.run('pui_browser_console', { level: 'debug' }));
  assert.equal(h.requests.length, 0);

  // Read-only and task workers cannot execute directly either.
  const ro = browserHost({ readonly: true });
  await assert.rejects(ro.run('pui_browser_read', {}), /cannot use browser automation/);
  assert.equal(ro.requests.length, 0);

  // Non-RPC/missing UI fails cleanly.
  const noUi = browserHost();
  noUi.ctx.mode = 'tui';
  await assert.rejects(noUi.run('pui_browser_console', {}), /native Pui/);
  assert.equal(noUi.requests.length, 0);
});

test('host errors and abort signals reject cleanly', async () => {
  const h = browserHost({ reply: async () => JSON.stringify({ error: 'No active Browser tab' }) });
  await assert.rejects(h.run('pui_browser_read', {}), /No active Browser tab/);

  const c = new AbortController();
  c.abort();
  await assert.rejects(h.run('pui_browser_read', {}, c.signal), /abort/i);
});

test('isLocalBrowserUrl allows loopback only and rejects spoofed or credentialed URLs', () => {
  for (const ok of [
    'http://localhost:3000',
    'https://LOCALHOST.:8443/app',
    'http://sub.localhost:5173',
    'http://127.0.0.1:8000',
    'http://127.2.3.4:9000',
    'http://[::1]:3000',
  ]) {
    assert.equal(isLocalBrowserUrl(ok), true, ok);
  }
  for (const bad of [
    'https://example.com',
    'https://localhost.example.com',
    'http://127.0.0.1.evil.com',
    'http://user:pass@localhost:3000',
    'javascript:alert(1)',
    'file:///tmp/a.html',
    'http://192.168.1.2:3000',
  ]) {
    assert.equal(isLocalBrowserUrl(bad), false, bad);
  }
});

test('permission gating across read-only, Plan, Auto, Manual, DontAsk, and Bypass modes', async () => {
  // 1. Read-only workers cannot use ANY browser automation tool in any mode.
  for (const mode of ['manual', 'plan', 'auto', 'bypassPermissions']) {
    const ro = permHost(mode, { readonly: true, approve: true, decision: 'allow' });
    for (const tool of BROWSER_TOOLS) {
      const res = await ro.gate(tool, { url: 'http://localhost:3000', kind: 'click', selector: 'a' });
      assert.equal(res?.block, true, `readonly ${mode} should block ${tool}`);
    }
    assert.equal(ro.prompts.length, 0);
    assert.equal(ro.calls.length, 0);
  }

  // 2. Plan mode: allows read/screenshot/console & localhost navigate; blocks act & non-localhost navigate.
  const plan = permHost('plan');
  assert.equal(await plan.gate('pui_browser_read', {}), undefined);
  assert.equal(await plan.gate('pui_browser_screenshot', {}), undefined);
  assert.equal(await plan.gate('pui_browser_console', {}), undefined);
  assert.equal(await plan.gate('pui_browser_navigate', { url: 'http://localhost:3000' }), undefined);
  assert.equal(await plan.gate('pui_browser_navigate', { url: 'http://127.0.0.1:8080/app' }), undefined);
  assert.equal((await plan.gate('pui_browser_navigate', { url: 'https://example.com' }))?.block, true);
  assert.equal((await plan.gate('pui_browser_act', { kind: 'click', selector: 'button' }))?.block, true);
  assert.equal(plan.prompts.length, 0);
  assert.equal(plan.calls.length, 0);

  // 3. Auto mode: read/screenshot/console & localhost navigate pre-allowed; remote navigate & act go to classifier.
  const auto = permHost('auto', { decision: 'allow' });
  assert.equal(await auto.gate('pui_browser_read', {}), undefined);
  assert.equal(await auto.gate('pui_browser_screenshot', {}), undefined);
  assert.equal(await auto.gate('pui_browser_console', {}), undefined);
  assert.equal(await auto.gate('pui_browser_navigate', { url: 'http://localhost:5173' }), undefined);
  assert.equal(await auto.gate('pui_browser_navigate', { url: 'http://127.0.0.1:3000' }), undefined);
  assert.equal(auto.calls.length, 0, 'read-only browser tools and localhost navigate should not invoke classifier');
  assert.equal(await auto.gate('pui_browser_navigate', { url: 'https://example.com' }), undefined);
  assert.equal(auto.calls.length, 1, 'external navigate should invoke Auto classifier');
  assert.equal(await auto.gate('pui_browser_act', { kind: 'click', selector: '#go' }), undefined);
  assert.equal(auto.calls.length, 2, 'browser_act should invoke Auto classifier');

  const autoDeny = permHost('auto', { decision: 'deny' });
  assert.equal((await autoDeny.gate('pui_browser_act', { kind: 'click', selector: '#delete' }))?.block, true);
  assert.equal((await autoDeny.gate('pui_browser_navigate', { url: 'https://example.com' }))?.block, true);

  // 4. Manual mode: read/screenshot/console allowed without prompt; navigate & act prompt user.
  const manual = permHost('manual', { approve: false });
  assert.equal(await manual.gate('pui_browser_read', {}), undefined);
  assert.equal(await manual.gate('pui_browser_screenshot', {}), undefined);
  assert.equal(await manual.gate('pui_browser_console', {}), undefined);
  assert.equal(manual.prompts.length, 0);
  assert.equal((await manual.gate('pui_browser_navigate', { url: 'http://localhost:3000' }))?.block, true);
  assert.equal(manual.prompts.length, 1);
  assert.equal((await manual.gate('pui_browser_act', { kind: 'click', selector: 'button' }))?.block, true);
  assert.equal(manual.prompts.length, 2);

  // 5. Bypass mode: all 5 allowed without prompts or classifier.
  const bypass = permHost('bypassPermissions');
  for (const tool of BROWSER_TOOLS) {
    assert.equal(await bypass.gate(tool, { url: 'https://example.com', kind: 'click', selector: 'a' }), undefined);
  }
  assert.equal(bypass.prompts.length, 0);
  assert.equal(bypass.calls.length, 0);
});
