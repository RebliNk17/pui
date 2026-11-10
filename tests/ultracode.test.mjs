import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, mkdir, writeFile, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

async function host(readonly = false, taskRole = '') {
  // Only type annotations/import resolution differ from the Pi extension host.
  const { stripTypeScriptTypes } = await import('node:module');
  let raw = await readFile(new URL('../scripts/ultracode.ts', import.meta.url), 'utf8');
  for (const name of ['permissions', 'compaction', 'rewind', 'questions', 'open']) {
    const dependency = stripTypeScriptTypes((await readFile(new URL(`../scripts/${name}.ts`, import.meta.url), 'utf8')).replace('import { Type } from "typebox";', 'const Type = new Proxy({}, { get: () => (...args) => ({args}) });'));
    raw = raw.replace(`"./${name}.ts"`, JSON.stringify(`data:text/javascript;base64,${Buffer.from(dependency).toString('base64')}`));
  }
  const source = stripTypeScriptTypes(raw.replace('import { Type } from "typebox";', 'const Type = new Proxy({}, { get: () => (...args) => ({args}) });'));
  const { default: extension, ULTRACODE_GUIDANCE } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
  const tools = new Map(), commands = new Map(), events = new Map(), statuses = [], messages = [], requests = [];
  let active=[];
  const pi = {appendEntry:()=>{},getAllTools:()=>[{name:"codemode"}],getActiveTools:()=>active,setActiveTools:tools=>{active=tools;}, registerCommand: (n, v) => commands.set(n, v), registerTool: t => tools.set(t.name, t), on: (n, f) => { const prev=events.get(n); events.set(n, prev ? async (...args) => { const first=await prev(...args); return await f(...args) ?? first; } : f); }, sendMessage: (...args) => messages.push(args) };
  const ctx = { modelRegistry:{getAvailable:()=>[]}, ui: { setStatus: (...args) => statuses.push(args), input: async (...args) => { requests.push(args); return JSON.stringify({ agent_id: 'agent-2' }); } } };
  const old = process.env.PUI_AGENT_ACCESS, oldRole = process.env.PUI_TASK_ROLE;
  process.env.PUI_AGENT_ACCESS = readonly ? 'read-only' : 'worktree'; process.env.PUI_TASK_ROLE = taskRole;
  try { extension(pi); } finally {
    if (old === undefined) delete process.env.PUI_AGENT_ACCESS; else process.env.PUI_AGENT_ACCESS = old;
    if (oldRole === undefined) delete process.env.PUI_TASK_ROLE; else process.env.PUI_TASK_ROLE = oldRole;
  }
  await events.get('session_start')({}, ctx);
  return { tools, commands, events, statuses, messages, requests, ctx, ULTRACODE_GUIDANCE };
}
test('Ultracode does not silently change model thinking or spawn a team', async () => {
  const h = await host();
  assert.equal(await h.events.get('before_agent_start')({systemPrompt:'base'}), undefined);
  await assert.rejects(h.tools.get('pui_spawn_agent').execute('', {}, undefined, null, h.ctx), /Enable Ultracode/);
  h.commands.get('pui-ultracode').handler('on', h.ctx);
  assert.match((await h.events.get('before_agent_start')({systemPrompt:'base'})).systemPrompt, /LOWEST useful number/);
  assert.match(h.ULTRACODE_GUIDANCE, /including zero/);
  assert.equal(h.requests.length, 0);
  assert.equal(h.messages.length, 0);
});
test('spawn asks the host and returns an ID, not a mirrored transcript', async () => {
  const h = await host(); h.commands.get('pui-ultracode').handler('on', h.ctx);
  const result = await h.tools.get('pui_spawn_agent').execute('call', {name:'reader',task:'Inspect models',access:'read-only'}, undefined, null, h.ctx);
  const request = JSON.parse(h.requests[0][1]); assert.equal(request.action,'spawn'); assert.equal(request.access,'read-only');
  assert.equal(result.details.pui.agent_id,'agent-2'); assert.equal(h.messages.length,0);
});
test('read-only workers cannot bypass restrictions through shell, codemode, or MCP', async () => {
  const h = await host(true); const gate = h.events.get('tool_call');
  for (const name of ['bash','write','edit','codemode','mcp__remote__mutate','pui_spawn_agent']) assert.equal((await gate({toolName:name},h.ctx)).block,true);
  for (const name of ['read','grep','find','ls','pui_message_agent','pui_ask_questions']) assert.equal(await gate({toolName:name},h.ctx),undefined);
});
test('verified-task builder is file-only even when permission mode is bypass', async () => {
  const h = await host(false, 'builder');
  const cwd = await mkdtemp(join(tmpdir(), 'pui-task-gate-'));
  await mkdir(join(cwd, 'src')); await writeFile(join(cwd, 'src/file'), 'original');
  Object.assign(h.ctx, {cwd, hasUI:true});
  h.commands.get('pui-settings').handler(JSON.stringify({permission:'bypassPermissions',compactPercent:85,keepRecentTokens:20000}), h.ctx);
  for (const toolName of ['bash','codemode','pui_spawn_agent','mcp__remote__write']) {
    assert.equal((await h.events.get('tool_call')({toolName,input:{command:'git commit -am unsafe'}},h.ctx)).block,true);
  }
  assert.equal(await h.events.get('tool_call')({toolName:'edit',input:{path:'src/file'}},h.ctx),undefined);
  for (const path of ['../outside','.git/config','.env']) assert.equal((await h.events.get('tool_call')({toolName:'write',input:{path}},h.ctx)).block,true);
  await symlink(tmpdir(),join(cwd,'escape'));
  assert.equal((await h.events.get('tool_call')({toolName:'write',input:{path:'escape/file'}},h.ctx)).block,true);
  assert.equal((await h.events.get('user_bash')({command:'pwd'},h.ctx)).result.exitCode,1);
});
test('task check runner still uses existing permission policy and has no model tools', async () => {
  const h = await host(false,'checks'); Object.assign(h.ctx,{hasUI:true,cwd:tmpdir()});
  h.commands.get('pui-settings').handler(JSON.stringify({permission:'manual',compactPercent:85,keepRecentTokens:20000}),h.ctx);
  h.ctx.ui.select=async()=> 'Deny';
  assert.equal((await h.events.get('user_bash')({command:'npm test'},h.ctx)).result.exitCode,1);
  assert.equal((await h.events.get('tool_call')({toolName:'write',input:{path:'file'}},h.ctx)).block,true);
});
test('explicit mailbox delivery is compact, visible and non-interrupting', async () => {
  const h = await host(); h.commands.get('pui-agent-message').handler(JSON.stringify({text:'Clarify the file ownership'}), h.ctx);
  assert.equal(h.messages.length,1); assert.equal(h.messages[0][1].deliverAs,'followUp'); assert.equal(h.messages[0][0].display,true);
  assert.throws(() => h.commands.get('pui-agent-message').handler(JSON.stringify({text:'x'.repeat(12001)}),h.ctx),/Invalid agent message/);
});
