import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';
import { existsSync } from 'node:fs';
const sdk = process.env.PUI_PI_SDK || `${process.env.HOME}/.pi/agent/install/releases/1.5.0/node_modules/@earendil-works/pi-coding-agent/dist/index.js`;
const source=stripTypeScriptTypes(await readFile(new URL('../scripts/compaction.ts',import.meta.url),'utf8'));
const { installCompaction }=await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
const fixture = Array.from({length:20},(_,i)=>({type:'message',id:`entry-${i}`,parentId:i?`entry-${i-1}`:null,timestamp:new Date(1700000000000+i*1000).toISOString(),message:{role:'user',content:[{type:'text',text:`Message ${i} `+'x'.repeat(1500)}],timestamp:1700000000000+i*1000}}));
function host(percent=70) {
  process.env.PUI_PI_SDK=sdk;
  const events=new Map(), statuses=[],calls=[];
  const pi={on:(n,f)=>events.set(n,f)};
  const ctx={model:{id:'fake',provider:'local',contextWindow:10000,maxTokens:1024,reasoning:false},thinkingLevel:'off',getContextUsage:()=>({tokens:8000,contextWindow:10000}),sessionManager:{getBranch:()=>fixture},modelRegistry:{streamSimple:(m,c,o)=>{calls.push({m,c,o});return {result:async()=>({role:'assistant',content:[{type:'text',text:'Summary of the earlier task.'}],stopReason:'stop',usage:{input:1,output:1,cacheRead:0,cacheWrite:0,totalTokens:2,cost:{input:0,output:0,cacheRead:0,cacheWrite:0,total:0}}})};}},ui:{setStatus:(...a)=>statuses.push(a)}};
  const control=installCompaction(pi,{compactPercent:percent,keepRecentTokens:1000});
  return {ctx,events,statuses,calls,control};
}
test('configured threshold uses actual Pi preparation and persists a canonical compaction draft without provider calls',{skip:!existsSync(sdk)},async()=>{
  const h=host();await h.events.get('session_start')({},h.ctx);
  assert.equal(JSON.parse(h.statuses.at(-1)[1]).error,null);
  const result=await h.events.get('turn_end')({outcome:'completed',entries:[]},h.ctx);
  assert.equal(result.entries[0].type,'compaction');
  assert.match(result.entries[0].summary,/Summary of the earlier task/);
  assert.ok(fixture.some(e=>e.id===result.entries[0].firstKeptEntryId));
  assert.equal(h.calls.length,1);assert.equal(h.calls[0].o.cacheRetention,'none');
  assert.equal(JSON.parse(h.statuses.at(-1)[1]).percent,70);
});
test('thresholds below configured percent cancel early Pi threshold; overflow is still summarized',{skip:!existsSync(sdk)},async()=>{
  const h=host(90);assert.equal(await h.events.get('turn_end')({outcome:'completed',entries:[]},h.ctx),undefined);
  assert.deepEqual(await h.events.get('session_before_compact')({reason:'threshold'},h.ctx),{cancel:true});
  const overflow=await h.events.get('session_before_compact')({reason:'overflow',branchEntries:fixture},h.ctx);
  assert.match(overflow.compaction.summary,/Summary/);
});
test('aborted turns never compact and invalid settings are rejected',{skip:!existsSync(sdk)},async()=>{
  const h=host();assert.equal(await h.events.get('turn_end')({outcome:'aborted'},h.ctx),undefined);
  assert.throws(()=>h.control.update({compactPercent:100,keepRecentTokens:1000},h.ctx),/Invalid/);
  assert.throws(()=>h.control.update({compactPercent:80,keepRecentTokens:-1},h.ctx),/Invalid/);
});
