import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';

const raw = (await readFile(new URL('../scripts/workflows.ts', import.meta.url), 'utf8'))
  .replace('import { Type } from "typebox";', 'const Type = new Proxy({}, { get: () => (...args) => ({args}) });');
const { installWorkflows } = await import(`data:text/javascript;base64,${Buffer.from(stripTypeScriptTypes(raw)).toString('base64')}`);
function host(entries = []) {
  let tool;
  const requests = [], statuses = [], saved = [], events = new Map();
  installWorkflows({ registerTool: t => { tool = t; }, on: (name, handler) => events.set(name, handler), appendEntry: (customType, data) => { const entry = {type:"custom", customType, data}; saved.push(entry); entries.push(entry); } });
  const ctx = { hasUI: true, mode: 'rpc', sessionManager: {getBranch: () => entries}, ui: {
    input: async (...args) => { requests.push(args); return JSON.stringify({ approved: false, regular_development: true }); },
    setStatus: (...args) => statuses.push(args),
  }};
  return { tool, ctx, requests, statuses, saved, events };
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

test('revision feedback is returned to the lead without approval, execution or hidden retry',async()=>{
 const h=host();h.ctx.ui.input=async()=>JSON.stringify({approved:false,revision_requested:true,feedback:'Keep public API unchanged',instruction:'Revise only the plan; do not implement'});
 const result=await h.tool.execute('',{...args,plan:concretePlan},undefined,null,h.ctx);
 assert.equal(result.details.workflow.approved,false);assert.equal(result.details.workflow.revision_requested,true);
 assert.equal(result.details.workflow.feedback,'Keep public API unchanged');assert.notEqual(result.terminate,true);
 assert.equal(h.statuses.length,1);assert.equal(h.requests.length,0);
});

test('workflow guidance excludes routine work and never treats Ultracode or planning as consent', () => {
  const h = host();
  const guidance = h.tool.promptGuidelines.join(' ');
  for (const text of ['Do not open a chat with a workflow offer', 'genuinely hard, high-risk', 'Routine fixes', 'NOT sufficient reasons', 'use subagents', 'Ultracode does not lower this threshold']) assert.ok(guidance.includes(text), text);
  assert.match(h.tool.description, /Do not offer for routine work/);
  assert.match(h.tool.promptSnippet, /otherwise develop normally/);
});

test('regular development suppresses subsequent offers and persists only a conversation preference', async () => {
  const h = host();
  await h.tool.execute('', args, undefined, null, h.ctx);
  const result = await h.tool.execute('', {...args, objective:'A different ordinary follow-up'}, undefined, null, h.ctx);
  assert.equal(h.requests.length, 1);
  assert.equal(result.details.workflow.offer_suppressed, true);
  assert.deepEqual(h.saved, [{type:'custom', customType:'pui-workflow-offers', data:{suppressed:true}}]);
  assert.equal(h.statuses.length, 1);
  const restored = host(h.saved.slice());
  restored.events.get('session_start')({}, restored.ctx);
  assert.equal((await restored.tool.execute('', args, undefined, null, restored.ctx)).details.workflow.offer_suppressed, true);
  assert.equal(restored.requests.length, 0);
  const separate = host();
  separate.events.get('session_start')({}, separate.ctx);
  await separate.tool.execute('', args, undefined, null, separate.ctx);
  assert.equal(separate.requests.length, 1);
});

test('only a positive explicit user workflow request can reopen native consent after decline', async () => {
  const h = host();
  await h.tool.execute('', args, undefined, null, h.ctx);
  for (const text of ['Continue', 'Implement everything', 'Use subagents', 'Make a plan', 'Do not use a workflow', "Don't start a workflow", 'Run without a workflow', 'Stop the workflow']) {
    h.events.get('input')({source:'rpc', text});
    assert.equal((await h.tool.execute('', args, undefined, null, h.ctx)).details.workflow.offer_suppressed, true, text);
  }
  h.events.get('input')({source:'extension', text:'Use a workflow'});
  assert.equal((await h.tool.execute('', args, undefined, null, h.ctx)).details.workflow.offer_suppressed, true);
  h.events.get('input')({source:'rpc', text:'Use a workflow for this migration'});
  assert.equal(h.requests.length, 1, 'explicit request alone runs nothing');
  h.ctx.ui.input = async (...request) => { h.requests.push(request); return JSON.stringify({approved:false,cancelled:true}); };
  const result = await h.tool.execute('', args, undefined, null, h.ctx);
  assert.equal(h.requests.length, 2, 'a fresh native consent is still required');
  assert.equal(result.details.workflow.approved, false);
  assert.equal(result.terminate, true);
  assert.deepEqual(h.saved.at(-1).data, {suppressed:false});
});

test('revision, cancellation and abort never persist a decline or execution authority', async () => {
  for (const response of [{approved:false,revision_requested:true}, {approved:false,cancelled:true}, undefined]) {
    const h = host();
    h.ctx.ui.input = async () => response === undefined ? undefined : JSON.stringify(response);
    await h.tool.execute('', args, undefined, null, h.ctx);
    assert.deepEqual(h.saved, []);
  }
  const h = host();
  await h.tool.execute('', args, undefined, null, h.ctx);
  const controller = new AbortController(); controller.abort();
  await assert.rejects(h.tool.execute('', args, controller.signal, null, h.ctx), /abort/i);
  assert.equal(h.requests.length, 1);
});
