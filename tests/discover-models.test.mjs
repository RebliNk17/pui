import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
const source = await readFile(new URL('../scripts/discover-models.mjs', import.meta.url), 'utf8');
const run = promisify(execFile);
async function discover(models, hub, { primary = 'https://gateway.test/v1/models', fallback = '', primaryFails = false, hubFails = false, groupInfo = null } = {}) {
  const mock = `
    const calls = [];
    globalThis.fetch = async (url, options) => {
      calls.push({url, options});
      if (String(url).endsWith('/model_group/info')) {
        return ${JSON.stringify(groupInfo)} === null
          ? {ok:false,status:403,text:async()=>''}
          : {ok:true,status:200,text:async()=>JSON.stringify(${JSON.stringify(groupInfo)})};
      }
      if (String(url).endsWith('/public/model_hub')) {
        if (${hubFails}) throw Error('hub unavailable');
        return {ok:true,status:200,text:async()=>JSON.stringify(${JSON.stringify(hub)})};
      }
      if (${primaryFails} && url === ${JSON.stringify(primary)}) return {ok:false,status:404,text:async()=>''};
      return {ok:true,status:200,text:async()=>JSON.stringify(${JSON.stringify(models)})};
    };
    process.on('exit', () => process.stderr.write(JSON.stringify(calls)));
  `;
  const { stdout, stderr } = await run(process.execPath, ['--input-type=module', '--eval', mock + source, primary, fallback, 'test-key']);
  const result = JSON.parse(stdout);
  return { result, body: JSON.parse(result.body), calls: JSON.parse(stderr) };
}
test('LiteLLM hub enriches matching models, preserves availability, limits and explicit flags', async () => {
  const { body, calls } = await discover({data:[
    {id:'standard',mode:'chat',max_input_tokens:1048576,max_output_tokens:65536},
    {id:'text-only',mode:'chat',max_input_tokens:200000,supports_reasoning:false,supports_vision:false},
    {id:'not-in-hub',mode:'chat',max_input_tokens:128000},
  ]}, [
    {model_group:'standard',supports_reasoning:true,supports_vision:true,max_input_tokens:null},
    {model_group:'text-only',supports_reasoning:true,supports_vision:true},
    {model_group:'not-available',supports_reasoning:true,supports_vision:true},
  ]);
  assert.equal(body.data.length,3);
  assert.equal(body.data[0].supports_reasoning,true);
  assert.equal(body.data[0].supports_vision,true);
  assert.equal(body.data[0].max_input_tokens,1048576);
  assert.equal(body.data[0].max_output_tokens,65536);
  assert.equal(body.data[1].supports_reasoning,false);
  assert.equal(body.data[1].supports_vision,false);
  assert.equal(body.data[2].supports_reasoning,undefined);
  assert.equal(calls[1].url,'https://gateway.test/model_group/info');
  assert.equal(calls[2].url,'https://gateway.test/public/model_hub');
  assert.equal(calls[0].options.headers.Authorization,'Bearer test-key');
  assert.equal(calls[1].options.headers.Authorization,'Bearer test-key');
  assert.deepEqual(calls[2].options.headers,{Accept:'application/json'});
  assert.equal(calls[1].options.redirect,'error');
  assert.equal(calls[2].options.redirect,'error');
});
test('metadata never overrides nested or alternate explicit capability fields', async () => {
  const rows=[
    {id:'nested',model_info:{supports_reasoning:false,supports_vision:false}},
    {id:'alternate',thinking:false,vision:false},
    {id:'input',reasoning:false,input:['text']},
    {id:'nested-reasoning',model_info:{reasoning:false}},
  ].map(m=>({...m,mode:'chat',max_input_tokens:1000000}));
  const {body}=await discover({data:rows},[],{groupInfo:{data:rows.map(m=>({model_group:m.id,supports_reasoning:true,supports_vision:true}))}});
  for(let i=0;i<3;i++) assert.deepEqual(body.data[i],rows[i]);
  assert.equal(body.data[3].supports_reasoning,undefined);
  assert.equal(body.data[3].supports_vision,true);
});
test('authenticated model group metadata takes precedence without public hub probe', async () => {
  const {body,calls}=await discover({data:[{id:'private-alias',mode:'chat',max_input_tokens:1000000}]},
    [{model_group:'private-alias',supports_reasoning:false,supports_vision:false}],
    {groupInfo:{data:[{model_group:'private-alias',supports_reasoning:true,supports_vision:true}]}});
  assert.equal(body.data[0].supports_reasoning,true);
  assert.equal(body.data[0].supports_vision,true);
  assert.equal(calls.length,2);
});
test('missing or malformed hub leaves original discovery usable', async () => {
  const models = {data:[{id:'alias',mode:'chat',max_input_tokens:1000000}]};
  for(const [hub, options] of [[null,{hubFails:true}], [{error:'unsupported'},{}], [[{model_group:'alias',supports_reasoning:null,supports_vision:'unknown'}],{}]]) {
    const {body,result}=await discover(models,hub,options);
    assert.equal(result.ok,true);
    assert.deepEqual(body,models);
  }
});
test('default-false group flags never erase known family capability', async () => {
  const {body}=await discover({data:[{id:'deepseek-v4-pro',mode:'chat',max_input_tokens:1048576}]},
    [{model_group:'deepseek-v4-pro',supports_reasoning:false,supports_vision:false}]);
  assert.equal(body.data[0].supports_reasoning,undefined);
  assert.equal(body.data[0].supports_vision,undefined);
});
test('ordinary OpenAI model lists do not trigger a metadata probe', async () => {
  const models={data:[{id:'plain-model'}]};
  const {body,calls}=await discover(models,[]);
  assert.deepEqual(body,models);assert.equal(calls.length,1);
});
test('fallback discovery and path-prefixed gateways retain matching hub path', async () => {
  const {body,calls}=await discover({data:[{id:'alias',mode:'chat',max_input_tokens:1000000}]},
    [{model_group:'alias',supports_reasoning:true,supports_vision:false}],
    {primary:'https://gateway.test/proxy/models',fallback:'https://gateway.test/proxy/v1/models',primaryFails:true});
  assert.equal(body.data[0].supports_reasoning,true);
  assert.equal(body.data[0].supports_vision,undefined,'default-false group metadata is unknown');
  assert.equal(calls[2].url,'https://gateway.test/proxy/model_group/info');
  assert.equal(calls[3].url,'https://gateway.test/proxy/public/model_hub');
});
