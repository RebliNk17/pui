import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';
const raw = await readFile(new URL('../scripts/open.ts', import.meta.url), 'utf8');
const source = stripTypeScriptTypes(raw.replace('import { Type } from "typebox";', 'const Type = new Proxy({}, { get: () => (...args) => ({args}) });'));
const { installOpen } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
function host(reply = async () => JSON.stringify({ opening: true })) {
  let tool;
  const requests = [];
  installOpen({ registerTool: value => { tool = value; } });
  const ctx = { hasUI: true, mode: 'rpc', cwd: '/workspace', ui: { input: (...args) => { requests.push(args); return reply(...args); } } };
  return { tool, ctx, requests, run: (target, signal) => tool.execute('call', { target }, signal, undefined, ctx) };
}
test('pui_open waits for host acknowledgment and resolves workspace paths without reading/uploading', async () => {
  let resolve;
  const h = host(() => new Promise(r => { resolve = r; }));
  let settled = false;
  const pending = h.run('docs/report.md').then(result => { settled = true; return result; });
  await Promise.resolve();
  assert.equal(settled, false);
  assert.equal(h.requests[0][0], 'Pui open');
  assert.deepEqual(JSON.parse(h.requests[0][1]), { version: 1, target: '/workspace/docs/report.md' });
  resolve(JSON.stringify({ opening: true }));
  const result = await pending;
  assert.deepEqual(result.details, { target: '/workspace/docs/report.md', opening: true });
  assert.match(result.content[0].text, /loading/);
  assert.match(h.tool.promptGuidelines[0], /instead of only printing/);
  assert.equal(h.tool.executionMode, 'sequential');
});
test('background previews report queued, not opened, and explain chat selection without parent retry', async () => {
  const h = host(async () => JSON.stringify({ queued: true }));
  const result = await h.run('/tmp/preview.html');
  assert.deepEqual(result.details, { target: '/tmp/preview.html', queued: true, opening: false });
  assert.match(result.content[0].text, /when the user selects this chat/);
  assert.match(result.content[0].text, /visible workspace was not changed/);
  assert.doesNotMatch(result.content[0].text, /Opened viewer/);
  assert.match(h.tool.description, /parent or child/);
  assert.match(h.tool.promptGuidelines[0], /do not claim it is already visible/);
});
test('absolute tmp reports, @paths, and localhost URLs are passed to the viewer', async () => {
  const h = host();
  for (const [target, expected] of [['/tmp/review.md', '/tmp/review.md'], ['@src/main.rs', '/workspace/src/main.rs'], ['http://localhost:3000', 'http://localhost:3000/']]) {
    assert.equal((await h.run(target)).details.target, expected);
  }
});
test('invalid schemes and embedded URL credentials never reach the host', async () => {
  const h = host();
  for (const target of ['', ' ', 'x\nfile', 'javascript:alert(1)', 'data:text/html,hi', 'file:///etc/passwd', 'https://user:password@example.com', 'https://']) await assert.rejects(h.run(target));
  assert.equal(h.requests.length, 0);
});
test('host errors, missing app, timeout and malformed acknowledgments are not success', async () => {
  for (const value of [undefined, '{}', 'not JSON', JSON.stringify({ error: 'Blocked sensitive file' }), JSON.stringify({ queued: 'true' }), JSON.stringify({ opening: true, queued: true }), JSON.stringify({ opening: true, queued: 'yes' }), JSON.stringify({ opening: 'yes', queued: true })]) await assert.rejects(host(async () => value).run('/tmp/report.md'));
  const h = host();
  h.ctx.mode = 'tui';
  await assert.rejects(h.run('report.md'), /native Pui/);
  assert.equal(h.requests.length, 0);
});
test('abort before or during opening is not reported as success', async () => {
  const controller = new AbortController();
  controller.abort();
  const h = host();
  await assert.rejects(h.run('/tmp/report.md', controller.signal), /abort/i);
  assert.equal(h.requests.length, 0);
  const c = new AbortController();
  const waiting = host((_title, _payload, { signal }) => new Promise(resolve => signal.addEventListener('abort', () => resolve(undefined), { once: true })));
  const promise = waiting.run('/tmp/report.md', c.signal);
  c.abort();
  await assert.rejects(promise, /abort/i);
});
