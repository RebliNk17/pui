import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, mkdir, writeFile, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

async function host(readonly = false, taskRole = '') {
  // Only type annotations/import resolution differ from the Pi extension host.
  const { stripTypeScriptTypes } = await import('node:module');
  let raw = await readFile(new URL('../scripts/ultracode.ts', import.meta.url), 'utf8');
  const urls = {};
  for (const name of ['permissions', 'compaction', 'context', 'titles', 'rewind', 'questions', 'open', 'visual', 'workflows', 'antigravity', 'accounts']) {
    let text = await readFile(new URL(`../scripts/${name}.ts`, import.meta.url), 'utf8');
    // Dependencies may import earlier adapter modules (accounts.ts → antigravity.ts).
    for (const [known, url] of Object.entries(urls)) text = text.replace(`"./${known}.ts"`, JSON.stringify(url));
    const dependency = stripTypeScriptTypes(text
      .replace('import { Type } from "typebox";', 'const Type = new Proxy({}, { get: () => (...args) => ({args}) });')
      .replace(/import\s*\{[^}]*\}\s*from\s*"@earendil-works\/pi-ai";/s, 'const createAssistantMessageEventStream = () => ({}), getCurrentSystemPrompt = () => "", getCurrentTools = () => [];'));
    urls[name] = `data:text/javascript;base64,${Buffer.from(dependency).toString('base64')}`;
    raw = raw.replace(`"./${name}.ts"`, JSON.stringify(urls[name]));
  }
  const source = stripTypeScriptTypes(raw.replace('import { Type } from "typebox";', 'const Type = new Proxy({}, { get: () => (...args) => ({args}) });'));
  const { default: extension, ULTRACODE_GUIDANCE } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
  const tools = new Map(), commands = new Map(), events = new Map(), statuses = [], messages = [], requests = [];
  let active=['read', 'bash', 'subagent', 'mcp__agents__spawn_agent'];
  const pi = {appendEntry:()=>{},getAllTools:()=>[{name:"codemode"}],getActiveTools:()=>active,setActiveTools:tools=>{active=tools;}, registerCommand: (n, v) => commands.set(n, v), registerTool: t => tools.set(t.name, t), on: (n, f) => { const prev=events.get(n); events.set(n, prev ? async (...args) => { const first=await prev(...args); if(n==='before_agent_start' && first?.systemPrompt) args[0]={...args[0],systemPrompt:first.systemPrompt}; return await f(...args) ?? first; } : f); }, sendMessage: (...args) => messages.push(args) };
  const ctx = { modelRegistry:{getAvailable:()=>[]}, ui: { setStatus: (...args) => statuses.push(args), input: async (...args) => { requests.push(args); return JSON.stringify({ agent_id: 'agent-2' }); } } };
  const old = process.env.PUI_AGENT_ACCESS, oldRole = process.env.PUI_TASK_ROLE, oldSettings = process.env.PUI_SETTINGS;
  process.env.PUI_AGENT_ACCESS = readonly ? 'read-only' : 'worktree'; process.env.PUI_TASK_ROLE = taskRole;
  // Hermetic: a host Pui chat's own settings (mode, picker scope) must not leak in.
  delete process.env.PUI_SETTINGS;
  try { extension(pi); } finally {
    if (oldSettings === undefined) delete process.env.PUI_SETTINGS; else process.env.PUI_SETTINGS = oldSettings;
    if (old === undefined) delete process.env.PUI_AGENT_ACCESS; else process.env.PUI_AGENT_ACCESS = old;
    if (oldRole === undefined) delete process.env.PUI_TASK_ROLE; else process.env.PUI_TASK_ROLE = oldRole;
  }
  await events.get('session_start')({}, ctx);
  return { tools, commands, events, statuses, messages, requests, ctx, ULTRACODE_GUIDANCE, active: () => active };
}
test('native Settings forwards the classifier override without changing the conversation model',async()=>{
  const h=await host();
  const candidate={id:'flash',name:'Flash',provider:'google',input:['text'],contextWindow:32000,maxTokens:1024,cost:{input:0.1,output:0.4}};
  h.ctx.modelRegistry.getAvailable=()=>[candidate];
  h.commands.get('pui-settings').handler(JSON.stringify({permission:'auto',permissionClassifier:'google/flash',compactPercent:85,keepRecentTokens:20000}),h.ctx);
  const status=JSON.parse(h.statuses.filter(([key])=>key==='pui-permissions').at(-1)[1]);
  assert.equal(status.classifier,'google/flash');assert.equal(status.classifierModels[0].id,'flash');
  assert.equal(h.requests.length,0);assert.equal(h.messages.length,0);
  const prompt=await h.events.get('before_agent_start')({systemPrompt:'base'});
  assert.match(prompt.systemPrompt,/Pui Auto mode/);assert.match(prompt.systemPrompt,/use pui_spawn_agent/);
});
test('Ultracode does not silently change model thinking or spawn a team', async () => {
  const h = await host();
  const prompt = (await h.events.get('before_agent_start')({systemPrompt:'base'})).systemPrompt;
  assert.match(prompt, /use pui_spawn_agent/);
  assert.match(prompt, /not registered with Pui/);
  assert.doesNotMatch(prompt, /Pui Ultracode is enabled/);
  assert.ok(h.active().includes('pui_spawn_agent'));
  assert.ok(h.active().includes('pui_wait_agents'));
  assert.ok(!h.active().includes('subagent'));
  assert.ok(!h.active().includes('mcp__agents__spawn_agent'));
  assert.ok(h.active().includes('bash'));
  h.commands.get('pui-ultracode').handler('on', h.ctx);
  assert.match((await h.events.get('before_agent_start')({systemPrompt:'base'})).systemPrompt, /LOWEST useful number/);
  assert.match(h.ULTRACODE_GUIDANCE, /including zero/);
  assert.equal(h.requests.length, 0);
  assert.equal(h.messages.length, 0);
});
test('ordinary chat guidance batches independent CLI checks without enabling tools or weakening safety', async () => {
  const h = await host();
  const before = h.active().slice();
  const { systemPrompt } = await h.events.get('before_agent_start')({systemPrompt:'base'});
  assert.match(systemPrompt, /Reduce model round trips/);
  assert.match(systemPrompt, /sibling tool calls concurrently/);
  assert.match(systemPrompt, /separate bash tool calls in the same batch/);
  assert.match(systemPrompt, /permission check, timeout, output and exit status/);
  assert.match(systemPrompt, /Keep dependent commands, edits to the same file, Git\/index mutations and shared build\/test resources ordered/);
  assert.match(systemPrompt, /never parallelize destructive actions or bypass approvals/);
  assert.match(systemPrompt, /Do not use detached\/background shell jobs or unmanaged agents/);
  assert.match(systemPrompt, /if codemode is active/);
  assert.deepEqual(h.active(), before);
  assert.equal(h.requests.length, 0);
  assert.equal(h.messages.length, 0);
  for (const [readonly, role] of [[true, ''], [false, 'builder'], [true, 'router'], [false, 'checks']]) {
    const worker = await host(readonly, role);
    const result = await worker.events.get('before_agent_start')({systemPrompt:'base'});
    assert.ok(!result?.systemPrompt?.includes('Reduce model round trips'));
  }
});
test('ordinary spawn uses the host, forwards another repo cwd and survives Ultracode off', async () => {
  const h = await host();
  for (const mode of ['off', 'on', 'off']) {
    h.commands.get('pui-ultracode').handler(mode, h.ctx);
    assert.ok(h.active().includes('pui_spawn_agent'));
    const result = await h.tools.get('pui_spawn_agent').execute('call', {name:'reader',task:'Inspect models',access:'read-only',cwd:'/other/repo'}, undefined, null, h.ctx);
    const request = JSON.parse(h.requests.at(-1)[1]);
    assert.equal(request.action,'spawn'); assert.equal(request.access,'read-only'); assert.equal(request.cwd,'/other/repo');
    assert.equal(result.details.pui.agent_id,'agent-2'); assert.equal(h.messages.length,0);
  }
});
test('read-only and task workers cannot activate or directly execute spawning', async () => {
  for (const [readonly, role] of [[true, ''], [false, 'builder'], [true, 'router'], [false, 'checks']]) {
    const h = await host(readonly, role);
    h.commands.get('pui-ultracode').handler('on', h.ctx);
    assert.ok(!h.active().includes('pui_spawn_agent'));
    await assert.rejects(h.tools.get('pui_spawn_agent').execute('', {}, undefined, null, h.ctx), /cannot create subagents/);
    assert.equal(h.requests.length, 0);
  }
});
test('automation tools are active without Ultracode and route bounded definitions to the host', async () => {
  const h = await host();
  for (const name of ['pui_create_monitor','pui_create_cron','pui_jobs','pui_update_job']) assert.ok(h.active().includes(name));
  await h.tools.get('pui_create_monitor').execute('', {name:'CI',command:'exit 0',interval_seconds:20,message:'Review CI'},undefined,null,h.ctx);
  assert.deepEqual(JSON.parse(h.requests.at(-1)[1]).kind,'monitor');
  assert.equal(JSON.parse(h.requests.at(-1)[1]).action,'automation_create');
  assert.equal(JSON.parse(h.requests.at(-1)[1]).command,'exit 0');
  await h.tools.get('pui_create_cron').execute('', {name:'Daily',schedule:'0 9 * * *',message:'Summarize'},undefined,null,h.ctx);
  assert.equal(JSON.parse(h.requests.at(-1)[1]).kind,'cron');
  assert.equal(JSON.parse(h.requests.at(-1)[1]).schedule,'0 9 * * *');
  await h.tools.get('pui_update_job').execute('', {job_id:'job',operation:'pause'},undefined,null,h.ctx);
  assert.equal(JSON.parse(h.requests.at(-1)[1]).action,'automation_update');
  assert.equal(h.messages.length,0);
  for (const [readonly,role] of [[true,''],[false,'builder'],[true,'router'],[false,'checks']]) {
    const worker=await host(readonly,role);
    for (const name of ['pui_create_monitor','pui_create_cron','pui_update_job']) {
      assert.ok(!worker.active().includes(name));
      await assert.rejects(worker.tools.get(name).execute('',{},undefined,null,worker.ctx),/cannot .*automation/);
    }
  }
});
test('managed spawn errors propagate without falling back to a detached process', async () => {
  const h = await host();
  h.ctx.ui.input = async () => JSON.stringify({error:'Plan mode cannot create workers'});
  await assert.rejects(h.tools.get('pui_spawn_agent').execute('', {}, undefined, null, h.ctx), /Plan mode/);
  assert.equal(h.messages.length, 0);
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
test('dynamic model router cannot invoke tools or shell even with bypass enabled', async () => {
  const h=await host(true,'router'); Object.assign(h.ctx,{cwd:tmpdir(),hasUI:true});
  h.commands.get('pui-settings').handler(JSON.stringify({permission:'bypassPermissions',compactPercent:85,keepRecentTokens:20000}),h.ctx);
  for(const toolName of ['read','bash','write','codemode','pui_ask_questions']) assert.equal((await h.events.get('tool_call')({toolName,input:{}},h.ctx)).block,true);
  assert.equal((await h.events.get('user_bash')({command:'pwd'},h.ctx)).result.exitCode,1);
});
test('agent mailbox steers busy recipients without aborting active tools', async () => {
  for (const readonly of [false, true]) {
    const h = await host(readonly);
    Object.assign(h.ctx, { isIdle: () => false, abort: () => assert.fail('mail must not abort active tools') });
    for (const text of ['Clarify the file ownership', 'Worker reader (agent-2) settled.\nBounded report with evidence']) {
      h.commands.get('pui-agent-message').handler(JSON.stringify({text}), h.ctx);
      assert.deepEqual(h.messages.at(-1), [
        {customType:'pui-agent-message',content:text,display:true},
        {triggerTurn:true,deliverAs:'steer'},
      ]);
    }
    assert.equal(h.messages.length,2);
    assert.throws(() => h.commands.get('pui-agent-message').handler(JSON.stringify({text:'x'.repeat(12001)}),h.ctx),/Invalid agent message/);
    assert.equal(h.messages.length,2);
  }
});
test('agent mailbox still wakes idle recipients', async () => {
  const h = await host();
  h.ctx.isIdle = () => true;
  h.commands.get('pui-agent-message').handler(JSON.stringify({text:'Review the findings'}), h.ctx);
  assert.equal(h.messages.length,1);
  assert.deepEqual(h.messages[0][1], {triggerTurn:true,deliverAs:'steer'});
});
