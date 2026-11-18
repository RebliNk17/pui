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
test('explicit pinned policy and reported stop threshold are forwarded for native consent', async () => {
  const h = host();
  await h.tool.execute('', {...args, policy: 'pinned', spend_limit: 1.25}, undefined, null, h.ctx);
  const request = JSON.parse(h.requests[0][1]);
  assert.equal(request.policy, 'pinned');
  assert.equal(request.spend_limit, 1.25);
  assert.equal(request.approved, undefined);
  const defaultHost = host();
  await defaultHost.tool.execute('', args, undefined, null, defaultHost.ctx);
  const defaultRequest = JSON.parse(defaultHost.requests[0][1]);
  assert.equal(defaultRequest.policy, undefined);
  assert.equal(defaultRequest.spend_limit, undefined);
});
test('invalid policy and nonpositive or nonfinite thresholds fail before a host question', async () => {
  const h = host();
  for (const policy of [null, '', 'automatic', 1, {}]) {
    await assert.rejects(h.tool.execute('', {...args, policy}, undefined, null, h.ctx), /policy/);
  }
  for (const spend_limit of [null, '5', 0, -1, NaN, Infinity, -Infinity]) {
    await assert.rejects(h.tool.execute('', {...args, spend_limit}, undefined, null, h.ctx), /threshold/);
  }
  assert.equal(h.requests.length, 0);
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

const concretePlan = { steps: ['Inspect focus handling', 'Add a regression'], checks: {criteria: [
  {id:'C1', description:'Keyboard retains the draft', command:'cargo test keyboard'}
]}};
test('concrete plan is forwarded intact for native approval without running it', async () => {
  const h = host();
  await h.tool.execute('', {...args, plan: concretePlan}, undefined, null, h.ctx);
  const request = JSON.parse(h.requests[0][1]);
  assert.deepEqual(request.plan, concretePlan);
  assert.equal(request.approved, undefined);
  assert.match(h.tool.promptGuidelines[0], /no redundant proposal agent/);
});
test('invalid or oversized plans fail before any host request', async () => {
  const h = host();
  for (const plan of [null, {}, {...concretePlan, steps: []}, {...concretePlan, steps:['x'.repeat(2001)]},
    {...concretePlan, approved:true}, {...concretePlan, checks:{criteria:[{id:'C1',description:'inspection only',command:''}]}},
    {...concretePlan, checks:{criteria:[concretePlan.checks.criteria[0],concretePlan.checks.criteria[0]]}}]) {
    await assert.rejects(h.tool.execute('', {...args, plan}, undefined, null, h.ctx), /plan|check|criteria/i);
  }
  assert.equal(h.requests.length, 0);
});
