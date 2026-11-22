import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { stripTypeScriptTypes } from 'node:module';
import { pathToFileURL } from 'node:url';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { once } from 'node:events';
const source = stripTypeScriptTypes(await readFile(new URL('../scripts/titles.ts', import.meta.url), 'utf8'));
const { installTitles, requestIdentity, TITLE_ENTRY } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);

function host({ entries = [], id = 'session-a', file = '/nonexistent/pui-title-fixture-a', reason = 'startup', name = '', parent, fail = false } = {}) {
  const events = new Map(), commands = new Map(), statuses = [], warnings = [];
  const sm = { getEntries: () => entries, getSessionId: () => id, getSessionFile: () => file, getSessionName: () => name, getHeader: () => ({ parentSession: parent }) };
  const pi = { on: (n, fn) => events.set(n, fn), registerCommand: (n, cmd) => commands.set(n, cmd), appendEntry: (customType, data) => { if (fail) throw Error('fixture'); entries.push({ type: 'custom', customType, data }); } };
  const ctx = { sessionManager: sm, ui: { setStatus: (key, value) => statuses.push({ key, ...JSON.parse(value) }), notify: (...args) => warnings.push(args) } };
  installTitles(pi);
  events.get('session_start')({ reason }, ctx);
  const prepare = (text = 'Fix Hebrew sidebar', overrides = {}) => commands.get('pui-initial-title').handler(JSON.stringify({ version: 1, sessionFile: file, sessionId: id, initialRequest: requestIdentity(text, 0), title: 'Fix Hebrew sidebar', ...overrides }), ctx);
  const input = (text = 'Fix Hebrew sidebar', source = 'rpc') => events.get('input')({ text, source }, ctx);
  return { events, commands, statuses, warnings, entries, ctx, prepare, input };
}

test('direct first-input identity assigns inert data once without modifying or sending text', () => {
  const h = host();
  h.prepare();
  assert.equal(h.entries.length, 0, 'Command acceptance is not persistence');
  const event = { text: 'Fix Hebrew sidebar', source: 'rpc' };
  assert.equal(h.events.get('input')(event, h.ctx), undefined);
  assert.deepEqual(event, { text: 'Fix Hebrew sidebar', source: 'rpc' });
  assert.equal(h.entries.length, 1);
  assert.equal(h.entries[0].customType, TITLE_ENTRY);
  assert.equal(h.statuses.at(-1).title, 'Fix Hebrew sidebar');
  assert.match(h.entries[0].data.initialRequest, /^[a-f0-9]{64}$/);
  assert.ok(!JSON.stringify(h.entries[0]).includes('text'));
  h.prepare('Later unrelated request'); h.input('Later unrelated request');
  assert.equal(h.entries.length, 1);
});

test('background/extension inputs, wrong path/id/request, duplicate preparations and unknown sessions cannot retitle', () => {
  for (const options of [
    { entries: [{ type: 'message', message: { role: 'user', content: 'Original question' } }] },
    { entries: [{ type: 'compaction', summary: 'Later context' }] },
    { entries: [{ type: 'session_info', name: 'Prior name' }] },
    { reason: 'reload' }, { reason: 'resume' }, { reason: 'fork' }, { parent: '/original' }, { name: 'Manual name' },
  ]) {
    const h = host(options), before = h.entries.length;
    h.prepare(); h.input(); assert.equal(h.entries.length, before); assert.equal(h.statuses.length, 0);
  }
  for (const overrides of [{ sessionFile: '/other' }, { sessionId: 'other' }, { title: 'x'.repeat(49) }, { title: 'Two\nlines' }]) {
    const h = host(); h.prepare(undefined, overrides); h.input(); assert.equal(h.entries.length, 0);
  }
  for (const source of ['extension', 'interactive']) {
    const h = host(); h.prepare(); h.input(undefined, source); assert.equal(h.entries.length, 0);
  }
  const h = host(); h.prepare(); h.prepare('Other scheduled request'); h.input('Other scheduled request');
  assert.equal(h.entries.length, 0); h.input(); assert.equal(h.entries.length, 0);
  const injected = host(); injected.input('Scheduled job fired'); injected.prepare(); injected.input();
  assert.equal(injected.entries.length, 0);
});

test('restart, compaction and fork preserve existing assignment and do not regenerate from projected context', () => {
  const h = host(); h.prepare(); h.input();
  for (const reason of ['startup', 'reload', 'resume', 'fork']) {
    const restored = host({ entries: [...h.entries, { type: 'compaction', summary: 'Unrelated later work' }], reason, id: 'fork-or-resume' });
    assert.equal(restored.statuses.at(-1).title, 'Fix Hebrew sidebar');
    restored.prepare('Later question'); restored.input('Later question'); assert.equal(restored.entries.length, 2);
  }
});

test('append failure, disconnect before raw input and shutdown do not publish an assigned title or retry', () => {
  const failed = host({ fail: true }); failed.prepare(); failed.input();
  assert.equal(failed.statuses.length, 0); assert.equal(failed.warnings.length, 1);
  failed.prepare(); failed.input(); assert.equal(failed.warnings.length, 1);
  const disconnected = host(); disconnected.prepare(); disconnected.events.get('session_shutdown')(); disconnected.input();
  assert.equal(disconnected.entries.length, 0);
  const restored = host({ reason: 'reload' }); restored.prepare(); restored.input(); assert.equal(restored.entries.length, 0);
});

test('an existing empty file without a marker is unknown, not an eligible new chat', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'pui-title-empty-'));
  try {
    const file = join(dir, 'empty.jsonl'); await writeFile(file, '');
    const h = host({ file }); h.prepare(); h.input(); assert.equal(h.entries.length, 0);
  } finally { await rm(dir, { recursive: true }); }
});

test('bundled helper is shipped and no standalone model/provider adapter is introduced', async () => {
  const bundle = await readFile(new URL('../scripts/bundle-macos.sh', import.meta.url), 'utf8');
  const smoke = await readFile(new URL('../scripts/smoke-runtime-macos.py', import.meta.url), 'utf8');
  assert.match(bundle, /scripts\/titles\.ts/); assert.match(smoke, /"titles\.ts"/);
  assert.doesNotMatch(source, /modelRegistry|sendUserMessage|setSessionName|fetch\(|writeFile|appendFile/);
});

test('real RPC dispatch prepares synchronously before the next input; setup never reaches provider', async () => {
  const sdk = process.env.PUI_PI_SDK;
  assert.ok(sdk);
  const dir = await mkdtemp(join(tmpdir(), 'pui-title-rpc-'));
  let child;
  try {
    const extension = join(dir, 'fixture.ts');
    await writeFile(extension, `import { installTitles } from ${JSON.stringify(new URL('../scripts/titles.ts', import.meta.url).pathname)};\nexport default pi => { installTitles(pi); pi.on('input', (event,ctx) => { ctx.ui.setStatus('fixture-input',JSON.stringify({text:event.text})); return {action:'handled'}; }); };`);
    const cli = new URL('./cli.js', pathToFileURL(sdk));
    child = spawn(process.execPath, [cli.pathname, '--mode', 'rpc', '--no-extensions', '--no-skills', '--no-prompt-templates', '--no-themes', '--extension', extension, '--session-dir', join(dir, 'sessions')], {
      cwd: dir, env: { PATH: process.env.PATH, HOME: dir, PI_CODING_AGENT_DIR: join(dir, 'agent') }, stdio: ['pipe', 'pipe', 'pipe'],
    });
    child.stderr.resume();
    const lines = createInterface({ input: child.stdout });
    const iterator = lines[Symbol.asyncIterator]();
    const records = [];
    const nextUntil = async predicate => {
      const timeout = setTimeout(() => child.kill(), 15000);
      try {
        for (;;) {
          const next = await iterator.next();
          assert.ok(!next.done, 'RPC exited before fixture result');
          const record = JSON.parse(next.value); records.push(record);
          if (predicate(record)) return record;
        }
      } finally { clearTimeout(timeout); }
    };
    child.stdin.write(JSON.stringify({ type: 'get_state', id: 'state' }) + '\n');
    const { data } = await nextUntil(r => r.id === 'state' && r.type === 'response');
    const text = 'Fix Hebrew sidebar';
    const setup = { version: 1, sessionFile: data.sessionFile, sessionId: data.sessionId, initialRequest: requestIdentity(text, 0), title: text };
    child.stdin.write([
      { type: 'prompt', id: 'prepare', message: '/pui-initial-title ' + JSON.stringify(setup) },
      { type: 'prompt', id: 'user', message: text },
    ].map(r => JSON.stringify(r) + '\n').join(''));
    await nextUntil(r => r.id === 'user' && r.type === 'response');
    const title = records.findIndex(r => r.statusKey === 'pui-initial-title');
    const input = records.findIndex(r => r.statusKey === 'fixture-input');
    assert.ok(title >= 0 && title < input, 'Title setup completed before actual raw input was handled');
    assert.equal(JSON.parse(records[input].statusText).text, text);
    assert.equal(records.filter(r => r.statusKey === 'fixture-input').length, 1, 'Setup did not become user/model input');
    assert.ok(!records.some(r => r.type === 'agent_start'), 'Fixture handles actual input before provider preflight');
    child.stdin.write(JSON.stringify({ type: 'prompt', id: 'bad', message: '/pui-initial-title {bad json' }) + '\n');
    await nextUntil(r => r.id === 'bad' && r.type === 'response');
    assert.equal(records.filter(r => r.statusKey === 'fixture-input').length, 1);
    lines.close();
  } finally {
    if (child && child.exitCode === null) { child.kill(); await once(child, 'exit'); }
    await rm(dir, { recursive: true });
  }
});

test('real public SessionManager persists marker with first user message and preserves through compaction/reopen', async () => {
  const sdk = process.env.PUI_PI_SDK;
  assert.ok(sdk, 'Run with installed PUI_PI_SDK; no provider required');
  const { SessionManager } = await import(pathToFileURL(sdk).href);
  const dir = await mkdtemp(join(tmpdir(), 'pui-title-sdk-'));
  try {
    const sm = SessionManager.create(dir, dir);
    const events = new Map(), commands = new Map(), statuses = [];
    installTitles({ on: (n, fn) => events.set(n, fn), registerCommand: (n, cmd) => commands.set(n, cmd), appendEntry: (type, data) => sm.appendCustomEntry(type, data) });
    const ctx = { sessionManager: sm, ui: { setStatus: (_, text) => statuses.push(JSON.parse(text)), notify: () => assert.fail('unexpected persistence failure') } };
    events.get('session_start')({ reason: 'startup' }, ctx);
    const file = sm.getSessionFile(), text = 'Fix Hebrew sidebar';
    commands.get('pui-initial-title').handler(JSON.stringify({ version: 1, sessionId: sm.getSessionId(), sessionFile: file, title: text, initialRequest: requestIdentity(text, 0) }), ctx);
    events.get('input')({ source: 'rpc', text }, ctx);
    await assert.rejects(readFile(file), { code: 'ENOENT' });
    assert.equal(statuses.at(-1).title, text);
    const first = sm.appendMessage({ role: 'user', content: text, timestamp: 1 });
    sm.appendCompaction('irrelevant summary', first, 10000);
    sm.appendSessionInfo('Mutable harness title');
    const reopened = SessionManager.open(file);
    assert.equal(reopened.getEntries().find(e => e.customType === TITLE_ENTRY).data.title, text);
    assert.ok((await readFile(file, 'utf8')).includes(TITLE_ENTRY));
  } finally { await rm(dir, { recursive: true }); }
});
