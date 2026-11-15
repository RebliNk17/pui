import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdtemp,mkdir,symlink} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {stripTypeScriptTypes} from 'node:module';
const source=stripTypeScriptTypes(await readFile(new URL('../scripts/rewind.ts',import.meta.url),'utf8'));
const {installRewind,restoreCheckpoints,workspacePath,navigateKeepingFiles}=await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
const state=(s)=>({data:Buffer.from(s).toString('base64'),mode:0o644});
async function host(){
 const cwd=await mkdtemp(join(tmpdir(),'pui-rewind-')), directory=join(cwd,'checkpoints');
 const {realpath}=await import('node:fs/promises');const root=await realpath(cwd);
 const events=new Map(),commands=new Map(),entries=[],statuses=[];let leaf=null;
 const pi={on:(n,f)=>events.set(n,f),registerCommand:(n,v)=>commands.set(n,v),appendEntry:(customType,data)=>{const entry={type:'custom',id:'c-'+entries.length,parentId:leaf,customType,data};entries.push(entry);leaf=entry.id;}};
 const ctx={cwd:root,isIdle:()=>true,hasPendingMessages:()=>false,ui:{setStatus:(key,value)=>statuses.push([key,JSON.parse(value)])},sessionManager:{getSessionFile:()=>join(cwd,'session.jsonl'),getBranch:()=>{const chain=[];let id=leaf;while(id){const e=entries.find(e=>e.id===id);if(!e)break;chain.unshift(e);id=e.parentId;}return chain;},getEntries:()=>entries},navigateTree:async(target)=>{leaf=entries.find(e=>e.id===target).parentId;return {cancelled:false};}};
 installRewind(pi,{directory});
 await events.get('session_start')({},ctx);
 const emit=(event,data={})=>events.get(event)?.(data,ctx);
 const user=async(id,timestamp,text)=>{await emit('message_start',{message:{role:'user',timestamp,content:text}});const entry={type:'message',id,parentId:leaf,message:{role:'user',timestamp,content:text}};entries.push(entry);leaf=id;};
 const result=()=>statuses.filter(([key])=>key==='pui-history-result').at(-1)?.[1];
 const points=()=>statuses.filter(([key])=>key==='pui-history').at(-1)[1].points;
 const command=async(operation,mode,entryId)=>{await commands.get('pui-history').handler(JSON.stringify({id:'request',operation,mode,entryId}),ctx);return result();};
 return {cwd:root,directory,ctx,entries,statuses,emit,user,points,command};
}
test('messages-only rewind preserves abandoned history and never reruns tools/prompts',async()=>{
 const h=await host();await h.user('u1',1,'original');await h.emit('agent_settled');await h.user('u2',2,'later');await h.emit('agent_settled');
 const before=h.entries.length;const r=await h.command('rewind','messages','u1');assert.equal(r.success,true);assert.equal(r.files,0);assert.ok(h.entries.length>before);assert.ok(h.entries.some(e=>e.id==='u2'));assert.equal(h.ctx.sessionManager.getBranch().some(e=>e.id==='u2'),false);
 assert.equal(h.entries.at(-1).customType,'pui-rewind');
});
test('tracked edits restore pre-existing dirty bytes and remove only exact new files',async()=>{
 const h=await host();await writeFile(join(h.cwd,'source.txt'),'already dirty');await h.user('u1',1,'update');
 await h.emit('tool_call',{toolName:'edit',toolCallId:'t1',input:{path:'source.txt'}});await writeFile(join(h.cwd,'source.txt'),'agent update');await h.emit('tool_result',{toolName:'edit',toolCallId:'t1'});
 await h.emit('tool_call',{toolName:'write',toolCallId:'t2',input:{path:'new.txt'}});await writeFile(join(h.cwd,'new.txt'),'created');await h.emit('tool_result',{toolName:'write',toolCallId:'t2'});await h.emit('agent_settled');
 assert.equal(h.points()[0].codeReason,null);
 const r=await h.command('rewind','both','u1');assert.equal(r.success,true);assert.equal(r.files,2);assert.equal(await readFile(join(h.cwd,'source.txt'),'utf8'),'already dirty');await assert.rejects(readFile(join(h.cwd,'new.txt')));assert.ok(r.recovery);
});
test('code-only keeps conversation; conflicting newer edits are never overwritten',async()=>{
 const h=await host();await writeFile(join(h.cwd,'file'),'before');await h.user('u1',1,'update');await h.emit('tool_call',{toolName:'edit',toolCallId:'t',input:{path:'file'}});await writeFile(join(h.cwd,'file'),'after');await h.emit('tool_result',{toolCallId:'t'});await h.emit('agent_settled');
 await writeFile(join(h.cwd,'file'),'manual update');let r=await h.command('rewind','code','u1');assert.equal(r.success,false);assert.match(r.error,/conflict/);assert.equal(await readFile(join(h.cwd,'file'),'utf8'),'manual update');assert.ok(h.ctx.sessionManager.getBranch().some(e=>e.id==='u1'));
 await writeFile(join(h.cwd,'file'),'after');r=await h.command('rewind','code','u1');assert.equal(r.success,true);assert.equal(await readFile(join(h.cwd,'file'),'utf8'),'before');assert.ok(h.ctx.sessionManager.getBranch().some(e=>e.id==='u1'));
});
test('missing checkpoints, shell/network effects, ambiguous paths and active runs block code restore',async()=>{
 const h=await host();await h.user('u1',1,'old');await h.emit('agent_settled');await h.user('u2',2,'run test');await h.emit('tool_call',{toolName:'bash',toolCallId:'b',input:{command:'npm test'}});await h.emit('agent_settled');
 assert.match(h.points()[0].codeReason,/untracked/);assert.match(h.points()[1].codeReason,/untracked/);
 let r=await h.command('rewind','both','u1');assert.equal(r.success,false);assert.match(r.error,/complete file checkpoint/);
 h.ctx.isIdle=()=>false;r=await h.command('rewind','messages','u1');assert.equal(r.success,false);assert.match(r.error,/Stop or finish/);
 for(const path of ['../outside','.git/config','.env.local'])await assert.rejects(workspacePath(h.cwd,path));
 await symlink(tmpdir(),join(h.cwd,'escape'));await assert.rejects(workspacePath(h.cwd,'escape/file'));
});
test('multi-message restore preflights every file and detects gaps between checkpoints',async()=>{
 const h=await host();await writeFile(join(h.cwd,'a'),'end');await writeFile(join(h.cwd,'b'),'external');
 const cp={version:1,cwd:h.cwd,userId:'u',changes:[{path:'a',before:state('start'),after:state('end')},{path:'b',before:state('start'),after:state('end')}]};
 await assert.rejects(restoreCheckpoints(h.cwd,[cp],h.directory),/conflict/);assert.equal(await readFile(join(h.cwd,'a'),'utf8'),'end');
 const c1={...cp,changes:[{path:'a',before:state('start'),after:state('mid')}]},c2={...cp,changes:[{path:'a',before:state('changed externally'),after:state('end')}]};
 await assert.rejects(restoreCheckpoints(h.cwd,[c1,c2],h.directory),/outside tracked/);assert.equal(await readFile(join(h.cwd,'a'),'utf8'),'end');
});
test('native Pi SessionManager fork includes selected assistant response and preserves source bytes/images',async()=>{
 const root=process.env.PUI_PI_SDK || process.env.HOME+'/.pi/agent/install/releases/1.5.0/node_modules/@earendil-works/pi-coding-agent/dist/index.js';
 const {SessionManager}=await import(root);const h=await host();const sessions=join(h.cwd,'sessions');await mkdir(sessions);
 const manager=SessionManager.create(h.cwd,sessions);manager.appendModelChange('fixture','test');const first=manager.appendMessage({role:'user',content:[{type:'text',text:'first'},{type:'image',data:'AA==',mimeType:'image/png'}],timestamp:1});const response=manager.appendMessage({role:'assistant',content:[{type:'text',text:'answer'}],timestamp:2,stopReason:'stop'});const second=manager.appendMessage({role:'user',content:'second',timestamp:3});
 const source=manager.getSessionFile();const before=await readFile(source,'utf8');const entries=manager.getEntries();
 h.ctx.sessionManager={getSessionFile:()=>source,getSessionDir:()=>sessions,getBranch:()=>manager.getBranch(),getEntries:()=>entries};
 const previous=process.env.PUI_PI_SDK;process.env.PUI_PI_SDK=root;
 try{const r=await h.command('fork','messages',response);assert.equal(r.success,true);assert.notEqual(r.path,source);assert.equal(await readFile(source,'utf8'),before);const fork=SessionManager.open(r.path);assert.ok(fork.getBranch().some(e=>e.id===first));assert.ok(fork.getBranch().some(e=>e.id===response),'Fork includes the selected response');assert.ok(!fork.getBranch().some(e=>e.id===second));
 const invalid=await h.command('fork','messages',first);assert.equal(invalid.success,false);assert.match(invalid.error,/completed assistant response/);}
 finally{if(previous===undefined)delete process.env.PUI_PI_SDK;else process.env.PUI_PI_SDK=previous;}
});

test('history points distinguish known unchanged ranges, edits and unknown effects',async()=>{
 const h=await host();await h.user('u1',1,'inspect');await h.emit('tool_call',{toolName:'read',toolCallId:'r',input:{path:'source'}});await h.emit('agent_settled');
 assert.equal(h.points()[0].fileChanges,0);
 await h.user('u2',2,'update');await h.emit('tool_call',{toolName:'write',toolCallId:'w',input:{path:'source'}});await writeFile(join(h.cwd,'source'),'after');await h.emit('tool_result',{toolCallId:'w'});await h.emit('agent_settled');
 assert.deepEqual(h.points().map(p=>p.fileChanges),[1,1]);
 await h.user('u3',3,'nothing');await h.emit('agent_settled');assert.deepEqual(h.points().map(p=>p.fileChanges),[1,1,0]);
 await h.user('u4',4,'shell');await h.emit('tool_call',{toolName:'bash',toolCallId:'b',input:{command:'unknown'}});await h.emit('agent_settled');
 assert.ok(h.points().every(p=>p.fileChanges===null));
});
test('no-op writes do not count as changes and legacy checkpoints remain unknown',async()=>{
 const h=await host();await writeFile(join(h.cwd,'same'),'unchanged');await h.user('u1',1,'no-op');await h.emit('tool_call',{toolName:'edit',toolCallId:'e',input:{path:'same'}});await h.emit('tool_result',{toolCallId:'e'});await h.emit('agent_settled');
 assert.equal(h.points()[0].fileChanges,0);
 delete h.entries.find(e=>e.customType==='pui-checkpoint').data.fileChanges;
 await h.emit('agent_settled');assert.equal(h.points()[0].fileChanges,null);
});
test('Pui navigation answers only redundant keep-files selection, never a restoring choice',async()=>{
 const h=await host();await h.user('u1',1,'original');await h.emit('agent_settled');
 const shown=[],select=async(title,options)=>{shown.push([title,options]);return options[0];};h.ctx.ui.select=select;
 const navigate=h.ctx.navigateTree;let duplicateAnswer;
 h.ctx.navigateTree=async(...args)=>{
  duplicateAnswer=await h.ctx.ui.select('Restore Options',['Keep current files','Restore files to that point','↩ Undo last rewind','Cancel navigation']);
  assert.equal(await h.ctx.ui.select('Other question',['A','B']),'A');
  assert.equal(await h.ctx.ui.select('Restore Options',['Custom restore action']),'Custom restore action');
  return navigate(...args);
 };
 assert.equal((await h.command('edit','messages','u1')).success,true);
 assert.equal(duplicateAnswer,'Keep current files');assert.equal(shown.length,2);assert.equal(h.ctx.ui.select,select);
 await h.ctx.ui.select('Restore Options',['Keep current files','Cancel navigation']);assert.equal(shown.length,3,'ordinary navigation remains interactive');
 h.ctx.navigateTree=async()=>{throw Error('navigation failed');};
 await assert.rejects(navigateKeepingFiles(h.ctx,'u1'),/navigation failed/);assert.equal(h.ctx.ui.select,select);
});
test('confirmed bounded file restore cannot trigger a second extension file restore',async()=>{
 const h=await host();await writeFile(join(h.cwd,'file'),'before');await h.user('u1',1,'edit');await h.emit('tool_call',{toolName:'edit',toolCallId:'e',input:{path:'file'}});await writeFile(join(h.cwd,'file'),'after');await h.emit('tool_result',{toolCallId:'e'});await h.emit('agent_settled');
 h.ctx.ui.select=async()=>{throw Error('duplicate dialog escaped');};const navigate=h.ctx.navigateTree;
 h.ctx.navigateTree=async(...args)=>{assert.equal(await readFile(join(h.cwd,'file'),'utf8'),'before');assert.equal(await h.ctx.ui.select('Restore Options',['Keep current files','Restore files to that point','Cancel navigation']),'Keep current files');return navigate(...args);};
 assert.equal((await h.command('edit','both','u1')).success,true);assert.equal(await readFile(join(h.cwd,'file'),'utf8'),'before');
});
