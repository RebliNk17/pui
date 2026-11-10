import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';

const raw = (await readFile(new URL('../scripts/workflows.ts', import.meta.url), 'utf8'))
  .replace('import { Type } from "typebox";', 'const Type = new Proxy({}, { get: () => (...args) => ({args}) });');
const { installWorkflows } = await import(`data:text/javascript;base64,${Buffer.from(stripTypeScriptTypes(raw)).toString('base64')}`);
function host() {
  let tool;
  const requests = [], statuses = [];
  installWorkflows({ registerTool: t => { tool = t; } });
  const ctx = { hasUI: true, mode: 'rpc', ui: {
    input: async (...args) => { requests.push(args); return JSON.stringify({ approved: false, regular_development: true }); },
    setStatus: (...args) => statuses.push(args),
  }};
  return { tool, ctx, requests, statuses };
}
const args = { objective: 'Preserve sidebar keyboard navigation', reason: 'Separate checks and review help catch focus regressions.' };
test('workflow is a sequential, consent-first host request, without an automatic start or timeout', async () => {
  const h = host();
  assert.equal(h.tool.executionMode, 'sequential');
  assert.match(h.tool.promptGuidelines[0], /Do not start a workflow automatically/);
  const result = await h.tool.execute('call', args, undefined, null, h.ctx);
  const [title, body, options] = h.requests[0];
  assert.equal(title, 'Pui agent control');
  assert.equal(JSON.parse(body).action, 'workflow_request');
  assert.equal(JSON.parse(body).reason, args.reason);
  assert.equal(options.timeout, undefined);
  assert.equal(result.details.workflow.regular_development, true);
  assert.equal(h.statuses[0][0], 'pui-workflow');
});
test('cancellation does not grant approval or invite an automatic retry', async () => {
  const h = host();
  h.ctx.ui.input = async () => undefined;
  const result = await h.tool.execute('call', args, undefined, null, h.ctx);
  assert.equal(result.terminate, true);
  assert.equal(result.details.cancelled, true);
  assert.match(result.content[0].text, /wait for the user's next instruction/);
  assert.equal(h.statuses.length, 1);
});
test('workflow request validates a short reason and fails closed outside native Pui', async () => {
  const h = host();
  await assert.rejects(h.tool.execute('', {...args, reason: 'x'.repeat(241)}, undefined, null, h.ctx), /very short reason/);
  await assert.rejects(h.tool.execute('', args, undefined, null, {...h.ctx, mode: 'tui'}), /native Pui/);
  assert.equal(h.requests.length, 0);
});
test('Stop aborts the wait and closes only its own request', async () => {
  const h = host();
  const controller = new AbortController();
  h.ctx.ui.input = async () => { controller.abort(); return '{}'; };
  await assert.rejects(h.tool.execute('', args, controller.signal, null, h.ctx), /abort/i);
  assert.equal(h.statuses.length, 1);
});
test('read-only and role workers cannot directly execute workflows', async () => {
  const h = host();
  const previous = process.env.PUI_TASK_ROLE;
  try {
    process.env.PUI_TASK_ROLE = 'builder';
    await assert.rejects(h.tool.execute('', args, undefined, null, h.ctx), /cannot start workflows/);
  } finally {
    if (previous === undefined) delete process.env.PUI_TASK_ROLE; else process.env.PUI_TASK_ROLE = previous;
  }
  assert.equal(h.requests.length, 0);
});
