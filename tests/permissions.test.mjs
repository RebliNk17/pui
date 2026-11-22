import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, mkdir, writeFile, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { stripTypeScriptTypes } from 'node:module';
import { pathToFileURL } from 'node:url';
const source=stripTypeScriptTypes(await readFile(new URL('../scripts/permissions.ts',import.meta.url),'utf8'));
const { installPermissions, cheapestModel, classifierModels, localPath, safeShell }=await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
const model=(id,provider,input,output)=>({id,provider,cost:{input,output},input:['text'],contextWindow:32000,maxTokens:2048});
const cwd=await mkdtemp(join(tmpdir(),'pui-permissions-'));
await mkdir(join(cwd,'src'));await writeFile(join(cwd,'src','file'),'test');
function host(mode, {decision='ask',fail=false,approve=false,readonly=false,response}={}) {
  const events=new Map(),prompts=[],calls=[],entries=[],statuses=[];
  const pi={on:(n,f)=>events.set(n,f),appendEntry:(...a)=>entries.push(a)};
  const ctx={
    cwd, hasUI:true, model:model('main','current',5,10),
    modelRegistry:{
      getAvailable:()=>[model('large','current',5,10),model('cheap','current',0.1,0.5),model('other','elsewhere',1,2)],
      streamSimple:(m,c,o)=>{
        calls.push({m,c,o});
        return {result:async()=>{
          if(fail)throw Error('secret auth error');
          return response ?? {content:[{type:'text',text:JSON.stringify({decision})}],stopReason:'stop',usage:{cost:{total:0.001}}};
        }};
      }
    },
    ui:{setStatus:(...a)=>statuses.push(a),confirm:async(...a)=>{prompts.push(a);return approve;}}
  };
  const control=installPermissions(pi,{permission:mode,readonly});
  events.get('before_agent_start')({messageType:'user',prompt:'Update src/file to fix the requested bug',systemPrompt:'base'});
  return {events,prompts,calls,entries,statuses,ctx,control,gate:(toolName,input={})=>events.get('tool_call')({toolName,input},ctx)};
}
test('unmanaged agent launches are rejected before approvals, classification or bypass', async () => {
  const commands = [
    'pi --print --mode json "fix bugs"',
    'pi -p "fix bugs"',
    'cd /other/repo && /Users/me/.pi/agent/bin/pi --mode rpc',
    'nohup pi --print "task" > events.jsonl &',
    'env MODEL=example pi --print task',
    'claude -p task',
    'codex exec task',
    'opencode run task',
    'python3 - <<\'PY\'\nargs=["/Users/me/.pi/agent/bin/pi","--print","--mode","json"]\nsubprocess.Popen(args,start_new_session=True)\nPY',
    'node -e \'spawn("pi", ["--print", "task"])\'',
    'node -e \'const s = await createAgentSession({}); await s.session.prompt("task")\'',
  ];
  for (const mode of ['manual', 'acceptEdits', 'plan', 'auto', 'dontAsk', 'bypassPermissions']) {
    const h = host(mode, {decision:'allow',approve:true});
    for (const command of commands) {
      const result = await h.gate('bash', {command});
      assert.equal(result?.block, true, `${mode}: ${command}`);
      assert.match(result.reason, /Use pui_spawn_agent/);
    }
    // Codemode's sandboxed wrapper follows ordinary permissions. The actual
    // nested Bash call is independently rejected, even with a parent tool ID.
    const nested = await h.events.get('tool_call')({toolName:'bash',parentToolCallId:'codemode-1',input:{command:'pi --print task'}}, h.ctx);
    assert.equal(nested.block, true);
    assert.equal((await h.gate('subagent', {task:'hidden worker'})).block, true);
    assert.equal((await h.gate('mcp__agents__spawn_agent', {task:'hidden worker'})).block, true);
    const shell = await h.events.get('user_bash')({command:'pi -p task'}, h.ctx);
    assert.equal(shell.result.exitCode, 1);
    assert.match(shell.result.output, /Use pui_spawn_agent/);
    assert.equal(h.prompts.length, 0);
    assert.equal(h.calls.length, 0);
  }
});
test('agent guard leaves managed tools, ordinary development and CLI discovery alone', async () => {
  const h = host('bypassPermissions');
  for (const command of ['pi --help', 'pi --version', 'pi auth check', 'pi --list-models', 'command -v pi', 'npm test', 'cargo test', 'python3 -c "import math; print(math.pi)"', 'rg "pi --print" README.md']) {
    assert.equal(await h.gate('bash', {command}), undefined, command);
  }
  assert.equal(await h.gate('pui_spawn_agent', {name:'reader',task:'Inspect',access:'read-only'}), undefined);
  assert.equal(await h.gate('codemode', {code:'await tools.pui_spawn_agent({name:"reader", task:"Inspect", access:"read-only"})'}), undefined);
  assert.equal(await h.gate('write', {path:'src/test',content:'subprocess.Popen(["pi", "--print"])'}), undefined);
  assert.equal(await h.gate('edit', {path:'src/test',newText:'const s = await createAgentSession({});'}), undefined);
  assert.equal(await h.gate('codemode', {code:'await tools.write({path:"src/test",content:\'subprocess.Popen(["pi", "--print"])\'})'}), undefined);
});
test('questions are available in all permission modes, including read-only, without approval or classification', async () => {
  for (const mode of ['manual', 'acceptEdits', 'plan', 'auto', 'dontAsk', 'bypassPermissions']) {
    for (const readonly of [false, true]) {
      const h = host(mode, { readonly });
      assert.equal(await h.gate('pui_ask_questions', { questions: [] }), undefined);
      assert.equal(h.prompts.length, 0);
      assert.equal(h.calls.length, 0);
    }
  }
});
test('local viewer opens are bounded host actions but web pages retain permission gates', async () => {
  for (const mode of ['manual', 'acceptEdits', 'plan', 'auto', 'dontAsk']) {
    const h = host(mode);
    assert.equal(await h.gate('pui_open', { target: '/tmp/review.md' }), undefined);
    assert.equal(await h.gate('pui_visual', { html: '<div>chart</div>' }), undefined);
    assert.equal(h.prompts.length, 0);
    assert.equal(h.calls.length, 0);
    assert.equal((await h.gate('pui_open', { target: 'https://example.com' })).block, true);
    const readonly = host(mode, { readonly: true });
    assert.equal((await readonly.gate('pui_open', { target: '/tmp/review.md' })).block, true);
    assert.equal((await readonly.gate('pui_visual', { html: '<div>chart</div>' })).block, true);
  }
});
test('workflow offer avoids a duplicate generic approval but never bypasses Plan, DontAsk or read-only', async () => {
  for (const mode of ['manual','auto','acceptEdits','bypassPermissions']) {
    const h = host(mode);
    assert.equal(await h.gate('pui_workflow', {objective:'Fix focus',reason:'Independent review catches regressions.'}), undefined);
    assert.equal(h.prompts.length, 0);
    assert.equal(h.calls.length, 0);
  }
  for (const mode of ['plan','dontAsk']) {
    assert.equal((await host(mode).gate('pui_workflow')).block, true);
  }
  assert.equal((await host('bypassPermissions', {readonly:true}).gate('pui_workflow')).block, true);
});
test('classifier selects cheapest authenticated text model across providers within scope',()=>{
  const h=host('auto');assert.equal(cheapestModel(h.ctx).id,'cheap');
  h.ctx.scopedModels=[{model:model('other','elsewhere',1,2)}];
  assert.equal(cheapestModel(h.ctx).id,'other');
  h.ctx.scopedModels=[{model:model('unavailable','missing',0,0)}];assert.equal(cheapestModel(h.ctx),undefined);
  h.ctx.scopedModels=[];
  const available=[model('haiku','anthropic',0.8,4),model('flash','google',0.1,0.4),model('opus','anthropic',5,25)];
  h.ctx.modelRegistry.getAvailable=()=>available;
  assert.equal(cheapestModel(h.ctx).id,'flash');
  available.splice(1,1);assert.equal(cheapestModel(h.ctx).id,'haiku');
});
test('selection excludes unsuitable/unknown prices, supports zero prices and deterministic ties',()=>{
  const h=host('auto');
  h.ctx.modelRegistry.getAvailable=()=>[
    {...model('image','x',0,0),input:['image']}, {...model('tiny','x',0,0),contextWindow:1024},
    {...model('short','x',0,0),maxTokens:128},model('unknown','x',undefined,1),model('negative','x',-1,1),model('infinite','x',0,Infinity),
    model('b','x',0,0),model('a','x',0,0),model('a','a',0,0)
  ];
  assert.equal(cheapestModel(h.ctx).provider,'a');
  h.ctx.modelRegistry.getAvailable=()=>[model('unknown','x',undefined,1)];
  assert.equal(cheapestModel(h.ctx),undefined);
  assert.equal(classifierModels(h.ctx).length,1,'unknown price can still be explicitly selected');
});
test('Auto override uses provider/model identity, updates live, and never silently falls back',async()=>{
  const h=host('auto',{decision:'allow'});
  h.control.update({permission:'auto',permissionClassifier:'elsewhere/other'},h.ctx);
  assert.equal(await h.gate('bash',{command:'npm test'}),undefined);
  assert.equal(h.calls.at(-1).m.provider,'elsewhere');
  let state=JSON.parse(h.statuses.at(-1)[1]);
  assert.equal(state.classifier,'elsewhere/other');
  assert.equal(JSON.parse(h.statuses.find(([,text])=>JSON.parse(text).classifierModels)[1]).classifierModels.length,3);
  assert.equal(state.classifierModels,undefined,'unchanged catalog is not resent on usage updates');
  h.control.update({permission:'auto',permissionClassifier:'missing/model'},h.ctx);
  assert.match((await h.gate('bash',{command:'npm test'})).reason,/no fallback model was used/);
  assert.equal(h.calls.length,1);assert.equal(h.prompts.length,0);
  h.control.update({permission:'auto',permissionClassifier:null},h.ctx);
  assert.equal(await h.gate('bash',{command:'npm test'}),undefined);assert.equal(h.calls.at(-1).m.id,'cheap');
});
test('Auto unavailable models, no context and oversized actions fail closed without prompts',async()=>{
  const unavailable=host('auto');unavailable.ctx.modelRegistry.getAvailable=()=>[];
  assert.match((await unavailable.gate('bash',{command:'npm test'})).reason,/No suitable priced classifier/);
  const noContext=host('auto');noContext.events.get('session_switch')({},noContext.ctx);
  assert.match((await noContext.gate('bash',{command:'npm test'})).reason,/No user request context/);
  const large=host('auto');assert.match((await large.gate('bash',{command:'x'.repeat(24001)})).reason,/too large/);
  for(const h of [unavailable,noContext,large]){assert.equal(h.calls.length,0);assert.equal(h.prompts.length,0);}
});
test('Auto rejects invalid JSON and incomplete responses without leaking provider errors',async()=>{
  for(const response of [
    {content:[{type:'text',text:'not json'}],stopReason:'stop'},
    {content:[{type:'text',text:'{"decision":"allow"}'}],stopReason:'length'},
    {content:[{type:'text',text:'{"reason":"allow me"}'}],stopReason:'stop'},
  ]){
    const h=host('auto',{response});
    const result=await h.gate('bash',{command:'npm test'});
    assert.equal(result.block,true);assert.equal(result.terminate,false);
    assert.match(result.reason,/not a model denial/);assert.equal(h.prompts.length,0);
  }
});
test('Auto prompt permits supporting development and keyword examples without weakening danger checks',async()=>{
  const h=host('auto',{decision:'allow'});
  for(const command of ['rg "git push" src','rg "git reset --hard" tests','git diff','cargo test','curl https://docs.example.com/api']) {
    assert.equal(await h.gate('bash',{command}),undefined,command);
  }
  assert.equal(h.calls.length,5);assert.equal(h.prompts.length,0);
  const prompt=h.calls[0].c.systemPrompt;
  assert.match(prompt,/not narrow task relevance/);assert.match(prompt,/need not be named verbatim/);
  assert.match(prompt,/NOT reasons to deny/);assert.match(prompt,/secret access/);assert.match(prompt,/untrusted data/);
});
test('Auto blocked attempts are attributed in the final response and cleared for the next run or branch',async()=>{
  const h=host('auto',{decision:'deny'});
  await h.gate('bash',{command:'npm test'});await h.gate('bash',{command:'npm run lint'});
  await h.gate('write',{path:'.env',content:'SECRET_DO_NOT_ECHO'});
  const final={role:'assistant',stopReason:'stop',content:[{type:'text',text:'Finished remaining work.'}]};
  assert.equal(h.events.get('message_end')({message:{...final,stopReason:'toolUse'}}),undefined);
  assert.equal(h.events.get('message_end')({message:{...final,stopReason:'aborted'}}),undefined);
  const result=h.events.get('message_end')({message:final});
  const text=JSON.stringify(result);
  assert.ok(text.includes('current/cheap denied (2 calls)'));assert.match(text,/built-in safety rule/);
  assert.match(text,/not executed/);assert.doesNotMatch(text,/SECRET_DO_NOT_ECHO/);
  assert.deepEqual(result.message.content[0],final.content[0]);
  h.events.get('before_agent_start')({prompt:'Next task',systemPrompt:'base'});
  assert.equal(h.events.get('message_end')({message:final}),undefined);
  await h.gate('bash',{command:'npm test'});
  h.events.get('session_tree')({},h.ctx);
  assert.equal(h.events.get('message_end')({message:final}),undefined);
});
test('Auto settings changes and aborts invalidate an in-flight classifier without approval escalation',async()=>{
  for(const change of ['model','mode','abort']){
    const h=host('auto',{decision:'allow'});
    const controller=new AbortController();h.ctx.signal=controller.signal;
    h.ctx.modelRegistry.streamSimple=()=>({result:async()=>{
      if(change==='abort')controller.abort();
      else h.control.update({permission:change==='mode'?'plan':'auto',permissionClassifier:'elsewhere/other'},h.ctx);
      return {content:[{type:'text',text:'{"decision":"allow"}'}],stopReason:'stop'};
    }});
    assert.equal((await h.gate('bash',{command:'npm test'})).block,true);
    assert.equal(h.prompts.length,0);
  }
});
test('real Pi agent loop continues after Auto denies a call and finishes with attributed blockers',async t=>{
  const sdk=process.env.PUI_PI_SDK || join(process.env.HOME,'.pi/agent/install/releases/1.5.0/node_modules/@earendil-works/pi-coding-agent/dist/index.js');
  const core=new URL('../../pi-agent-core/dist/index.js',pathToFileURL(sdk));
  try {await readFile(core);}
  catch {t.skip('Installed Pi SDK is unavailable');return;}
  const {Agent}=await import(core.href);
  const h=host('auto',{decision:'deny'}),executed=[],requests=[];
  const tools=['bash','read'].map(name=>({name,description:name,parameters:{type:'object',properties:{command:{type:'string'},path:{type:'string'}}},execute:async()=>{
    executed.push(name);return {content:[{type:'text',text:'read succeeded'}],details:{}};
  }}));
  const agent=new Agent({
    initialState:{model:h.ctx.model,tools,systemPrompt:'Complete safe work; report blockers.'},
    beforeToolCall:async({toolCall,args})=>h.events.get('tool_call')({toolName:toolCall.name,input:args},h.ctx),
    streamFn:(_model,context)=>{
      requests.push(context);
      const step=requests.length;
      assert.ok(step<=3,'denial must not cause a retry loop');
      const content=step===1?[{type:'toolCall',id:'blocked',name:'bash',arguments:{command:'npm test'}}]:step===2?[{type:'toolCall',id:'safe',name:'read',arguments:{path:'src/file'}}]:[{type:'text',text:'Finished the remaining inspection.'}];
      const message={role:'assistant',content,stopReason:step<3?'toolUse':'stop',api:'test',provider:'current',model:'main',timestamp:Date.now(),usage:{input:0,output:0,cacheRead:0,cacheWrite:0,totalTokens:0,cost:{input:0,output:0,cacheRead:0,cacheWrite:0,total:0}}};
      return {async *[Symbol.asyncIterator](){},result:async()=>message};
    }
  });
  // Match AgentSession's message_end replacement contract, without credentials/network.
  agent.subscribe(event=>{
    if(event.type==='message_end'){
      const replacement=h.events.get('message_end')({message:event.message});
      if(replacement)Object.assign(event.message,replacement.message);
    }
  });
  await agent.prompt('Inspect and test the change');
  assert.equal(requests.length,3);assert.deepEqual(executed,['read']);assert.equal(h.prompts.length,0);
  const denied=agent.state.messages.find(m=>m.role==='toolResult'&&m.toolCallId==='blocked');
  assert.equal(denied.isError,true);assert.match(denied.content[0].text,/Continue remaining safe work/);
  assert.ok(JSON.stringify(agent.state.messages.at(-1)).includes('current/cheap denied'));
});
test('Manual reads without prompts but asks on every mutating call',async()=>{
  const h=host('manual',{approve:true});assert.equal(await h.gate('read',{path:'src/file'}),undefined);
  for(let i=0;i<2;i++)assert.equal(await h.gate('edit',{path:'src/file'}),undefined);
  assert.equal(h.prompts.length,2);assert.equal(h.calls.length,0);
  const d=host('manual');assert.equal((await d.gate('bash',{command:'pwd'})).block,true);
});
test('Accept edits is workspace-only with canonical symlink and protected path checks',async()=>{
  const h=host('acceptEdits');assert.equal(await h.gate('edit',{path:'src/file'}),undefined);
  assert.equal(await h.gate('write',{path:'src/new/deep/file'}),undefined);
  assert.equal(await h.gate('bash',{command:'mkdir -p src/new'}),undefined);
  assert.equal(await h.gate('bash',{command:'cp src/file src/copy'}),undefined);
  for(const path of ['../outside','/etc/hosts','.pi/settings.json','.git/config','.env.local'])assert.equal((await h.gate('write',{path})).block,true);
  await symlink(tmpdir(),join(cwd,'escape'));assert.equal(await localPath(cwd,'escape/file'),false);
  assert.equal((await h.gate('bash',{command:'npm test'})).block,true);
  assert.equal((await h.gate('mcp__network__post',{})).block,true);
});
test('Plan blocks writes, scripts, codemode and delegation regardless of approval',async()=>{
  const h=host('plan',{approve:true});
  for(const name of ['write','edit','codemode','mcp__remote__mutate','pui_spawn_agent','pui_message_agent'])assert.equal((await h.gate(name,{path:'src/file'})).block,true);
  for(const command of ['pwd','ls -al','uname -s'])assert.equal(await h.gate('bash',{command}),undefined);
  for(const command of ['ls; touch hacked','ls $(touch hacked)','git diff','find . -exec sh {} \\;','python -c pass','ls > file'])assert.equal((await h.gate('bash',{command})).block,true);
  assert.equal(h.prompts.length,0);assert.equal(h.calls.length,0);
});
test('read-only workers can report to their parent in every mode without gaining mutation tools', async () => {
  for (const mode of ['manual', 'acceptEdits', 'plan', 'auto', 'dontAsk', 'bypassPermissions']) {
    const h = host(mode, {readonly:true});
    assert.equal(await h.gate('pui_message_agent', {agent_id:'parent',message:'Blocker found'}), undefined);
    for (const tool of ['bash','write','edit','pui_spawn_agent','pui_stop_agent','codemode']) {
      assert.equal((await h.gate(tool, {})).block, true, `${mode}: ${tool}`);
    }
    assert.equal(h.prompts.length, 0);
    assert.equal(h.calls.length, 0);
  }
});
test('Don’t ask grants bounded local operations and denies all prompts',async()=>{
  const h=host('dontAsk',{approve:true});
  assert.equal(await h.gate('write',{path:'src/local'}),undefined);
  assert.equal((await h.gate('bash',{command:'curl https://example.com'})).block,true);
  assert.equal((await h.gate('codemode',{code:'tools.read({path:"src/file"})'})).block,true);
  assert.equal(h.prompts.length,0);assert.equal(h.calls.length,0);
});
test('Auto actually calls cheapest classifier and records separate cost',async()=>{
  const h=host('auto',{decision:'allow'});assert.equal(await h.gate('bash',{command:'npm test'}),undefined);
  assert.equal(h.calls.length,1);assert.equal(h.calls[0].m.id,'cheap');assert.equal(h.calls[0].c.tools,undefined);
  assert.equal(h.entries[0][0],'pui-permission-check');assert.equal(h.prompts.length,0);
  assert.equal(JSON.parse(h.statuses.at(-1)[1]).classifierCost,0.001);
});
test('Auto denial, uncertainty, errors and malformed decisions continue without any approval',async()=>{
  for(const options of [{decision:'deny'},{decision:'ask'},{decision:'invalid'},{fail:true}]){
    const h=host('auto',{...options,approve:true});
    const result=await h.gate('bash',{command:'npm test'});
    assert.equal(result.block,true);assert.equal(result.terminate,false);assert.equal(h.prompts.length,0);
    assert.match(result.reason,/Continue remaining safe work/);
    assert.match(result.reason,/without asking the user/);
    assert.doesNotMatch(result.reason,/secret auth error/);
    assert.equal(await h.gate('edit',{path:'src/file'}),undefined,'remaining safe work continues');
    assert.equal(await h.gate('read',{path:'src/file'}),undefined);
  }
});
test('Auto permits canonical workspace edits even when code mentions dangerous words',async()=>{
  const h=host('auto',{fail:true});
  for(const path of ['src/file','src/new-test'])assert.equal(await h.gate('edit',{path,newText:'delete button; git push documentation; rm tests'}),undefined);
  assert.equal(await h.gate('bash',{command:'mkdir -p src/generated'}),undefined);
  assert.equal(h.calls.length,0);assert.equal(h.prompts.length,0);
  assert.equal((await h.gate('write',{path:'.env.local',content:'example'})).block,true);
  assert.equal(h.prompts.length,0);
});
test('Auto retains original task context across short followups and checks codemode wrappers',async()=>{
  const h=host('auto',{decision:'allow'});
  h.events.get('input')({source:'rpc',text:'done'});
  h.events.get('before_agent_start')({prompt:'done',systemPrompt:'base'});
  assert.equal(await h.gate('codemode',{code:'await tools.edit({path:"src/file",newText:"delete button"})'}),undefined);
  const payload=JSON.parse(h.calls[0].c.messages[0].content);
  assert.deepEqual(payload.RECENT_USER_REQUESTS,['Update src/file to fix the requested bug','done']);
  assert.match(h.calls[0].c.systemPrompt,/DANGEROUS EFFECTS/);
  assert.equal(h.calls[0].o.reasoning,undefined);
});
test('Auto dangerous commands and external mutations are blocked without asking',async()=>{
  const h=host('auto',{decision:'allow'});
  for(const command of ['rm -rf src','git push origin main','git reset --hard','kubectl delete pod production','terraform apply'])assert.equal((await h.gate('bash',{command})).block,true);
  assert.equal((await h.gate('mcp__cloud__delete_resource',{resource:'shared'})).block,true);
  assert.equal(h.calls.length,0);assert.equal(h.prompts.length,0);
});
test('Auto credential reads block without asking instead of using the routine-read allowlist',async()=>{
  const h=host('auto',{decision:'allow'});
  for(const path of ['.env.local','/home/user/.ssh/id_ed25519','/home/user/.pi/agent/auth.json','/home/user/.aws/credentials'])assert.equal((await h.gate('read',{path})).block,true);
  assert.equal(h.calls.length,0);assert.equal(h.prompts.length,0);
  assert.equal(await h.gate('read',{path:'src/file'}),undefined);
});
test('Bypass skips approval and classification but never read-only worker policy',async()=>{
  const h=host('bypassPermissions');
  for(const [tool,input] of [['bash',{command:'rm -rf src'}],['write',{path:'/outside/workspace'}],['codemode',{code:'unrestricted'}],['mcp__remote__delete',{}]])assert.equal(await h.gate(tool,input),undefined);
  assert.equal(h.calls.length,0);assert.equal(h.prompts.length,0);
  const readonly=host('bypassPermissions',{readonly:true});
  for(const tool of ['write','bash','codemode','mcp__remote__delete'])assert.equal((await readonly.gate(tool,{})).block,true);
  assert.equal(await readonly.gate('read',{path:'src/file'}),undefined);
});
test('No UI fails closed; user shell commands use the same gate',async()=>{
  const h=host('manual');h.ctx.hasUI=false;assert.equal((await h.gate('write',{path:'src/file'})).block,true);assert.equal(h.prompts.length,0);
  const plan=host('plan');const r=await plan.events.get('user_bash')({command:'touch hacked'},plan.ctx);assert.equal(r.result.exitCode,1);
  assert.equal(safeShell('pwd\nrm -rf src'),false);
});
test('same mode settings updates keep current approvals valid; mode changes cancel pending asks',async()=>{
  const h=host('manual',{approve:true});h.ctx.ui.confirm=async()=>{h.control.update('manual',h.ctx);return true;};
  assert.equal(await h.gate('write',{path:'src/file'}),undefined);
  const pending=host('manual');let awaiting;
  const ready=new Promise(resolve=>{awaiting=resolve;});
  pending.ctx.ui.confirm=async(_title,_message,{signal})=>{awaiting();return await new Promise(resolve=>signal.addEventListener('abort',()=>resolve(false),{once:true}));};
  const call=pending.gate('write',{path:'src/file'});await ready;
  pending.control.update('plan',pending.ctx);assert.equal((await call).block,true);
});
test('Always allow is exact-action session memory and never grants an entire tool',async()=>{
  const h=host('manual');let selections=0;
  h.ctx.ui.select=async(title,options)=>{selections++;assert.match(title,/Approve this exact action/);return options[1];};
  assert.equal(await h.gate('bash',{command:'npm test',timeout:120}),undefined);
  assert.equal(await h.gate('bash',{timeout:120,command:'npm test'}),undefined);
  assert.equal(selections,1,'object key order should not change an exact action');
  assert.equal(await h.gate('bash',{command:'npm test --all',timeout:120}),undefined);
  assert.equal(selections,2,'different command requires a new choice');
  h.control.update('acceptEdits',h.ctx);
  assert.equal(await h.gate('bash',{command:'npm test',timeout:120}),undefined);assert.equal(selections,3);
  h.events.get('session_switch')({},h.ctx);
  assert.equal(await h.gate('bash',{command:'npm test',timeout:120}),undefined);assert.equal(selections,4);
  const readonly=host('manual',{readonly:true});readonly.ctx.ui.select=async()=>{throw Error('must not ask');};
  assert.equal((await readonly.gate('bash',{command:'npm test'})).block,true);
});
test('Cancel/deny and mode changes cannot install an always-allow grant',async()=>{
  const h=host('manual');h.ctx.ui.select=async()=>undefined;
  assert.equal((await h.gate('bash',{command:'npm test'})).block,true);
  h.ctx.ui.select=async()=>{h.control.update('plan',h.ctx);return 'Always allow this exact action (this chat)';};
  assert.equal((await h.gate('bash',{command:'npm test'})).block,true);
  assert.equal((await h.gate('bash',{command:'npm test'})).block,true);
});
test('automation authorizes future runs explicitly without AI classification and guards unmanaged agents',async()=>{
  for (const name of ['pui_create_monitor','pui_create_cron','pui_update_job']) {
    const args={name:'Check',command:'curl https://example.com/health',schedule:'0 9 * * *',message:'Review',operation:'resume'};
    const denied=host('auto',{decision:'allow',approve:false});
    assert.equal((await denied.gate(name,args)).block,true);
    assert.equal(denied.calls.length,0); assert.equal(denied.prompts.length,1);
    const allowed=host('auto',{approve:true});
    assert.equal(await allowed.gate(name,args),undefined); assert.equal(allowed.calls.length,0);
    assert.equal((await host('plan').gate(name,args)).block,true);
    assert.equal((await host('dontAsk').gate(name,args)).block,true);
  }
  for (const mode of ['auto','bypassPermissions']) {
    const h=host(mode,{approve:true});
    assert.equal((await h.gate('pui_create_monitor',{command:'pi --print do stuff'})).block,true);
    assert.equal(h.calls.length,0); assert.equal(h.prompts.length,0);
  }
  assert.equal(await host('plan').gate('pui_jobs',{}),undefined);
});
test('mode changes invalidate outstanding approval',async()=>{
  const h=host('manual',{approve:true});h.ctx.ui.confirm=async()=>{h.control.update('plan',h.ctx);return true;};
  assert.equal((await h.gate('write',{path:'src/file'})).block,true);
});

test('Plan workflow handoff only opens native review; it never grants tool permissions', async () => {
  const h = host('plan', { approve: true, decision:'allow' });
  assert.equal(await h.gate('pui_workflow', {plan:{steps:['Inspect then fix'], checks:{criteria:[{id:'C1',description:'Pass tests',command:'npm test'}]}}}), undefined);
  assert.equal(h.calls.length,0); assert.equal(h.prompts.length,0);
  assert.equal((await h.gate('pui_workflow', {})).block,true);
  assert.equal((await h.gate('write', {path:'src/file', content:'blocked'})).block,true);
  assert.equal((await h.events.get('user_bash')({command:'npm test'},h.ctx)).result.exitCode,1);
  const readonly=host('plan',{readonly:true});
  assert.equal((await readonly.gate('pui_workflow',{plan:{steps:['x']}})).block,true);
  const noAsk=host('dontAsk');
  assert.equal((await noAsk.gate('pui_workflow',{plan:{steps:['x']}})).block,true);
});
