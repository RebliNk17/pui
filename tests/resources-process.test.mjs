import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';

let sdkPath = process.env.PUI_PI_SDK;
if (!sdkPath) {
  const agentDir = process.env.PI_CODING_AGENT_DIR ?? path.join(os.homedir(), '.pi/agent');
  try { sdkPath = path.join(agentDir, 'install/releases', fs.readFileSync(path.join(agentDir, 'install/current-version'), 'utf8').trim(), 'node_modules/@earendil-works/pi-coding-agent/dist/index.js'); } catch {}
}
function helper(cwd, agentDir, request) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ['--input-type=module', '--eval', fs.readFileSync('scripts/resources.mjs', 'utf8'), sdkPath], {
      cwd, env: { ...process.env, PI_CODING_AGENT_DIR: agentDir, PUI_RESOURCES_HELPER: '1' }, stdio: ['pipe', 'pipe', 'pipe'], timeout: 20000,
    });
    let stdout = '';
    child.stdout.on('data', bytes => { stdout += bytes; });
    child.stderr.resume();
    child.on('error', reject);
    child.on('close', code => { try { assert.equal(code, 0); resolve(JSON.parse(stdout)); } catch (error) { reject(error); } });
    child.stdin.end(JSON.stringify(request));
  });
}
test('embedded helper protocol survives local package installation and does not leak MCP secrets', { skip: !sdkPath || !fs.existsSync(sdkPath) }, async () => {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'pui-resource-process-')));
  const cwd = path.join(root, 'workspace');
  const agentDir = path.join(root, 'agent');
  const pkg = path.join(root, 'fixture-package');
  for (const dir of [cwd, agentDir, path.join(pkg, 'prompts')]) fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(pkg, 'package.json'), '{"name":"pui-local-fixture","pi":{"prompts":["prompts/review.md"]}}');
  fs.writeFileSync(path.join(pkg, 'prompts/review.md'), '---\ndescription: Review fixture\n---\nReview');
  fs.writeFileSync(path.join(agentDir, 'mcp.json'), '{"mcpServers":{"fixture":{"url":"https://invalid.example/mcp?token=fixture-secret","headers":{"Authorization":"Bearer fixture-secret"}}}}');
  let { data, error } = await helper(cwd, agentDir, { action: 'list' });
  assert.equal(error, undefined);
  assert.ok(!JSON.stringify(data).includes('fixture-secret'));
  let result = await helper(cwd, agentDir, { action: 'install', scope: 'user', source: pkg, revision: data.revision });
  assert.equal(result.error, undefined);
  assert.equal(result.data.changed, true);
  ({ data } = await helper(cwd, agentDir, { action: 'list' }));
  assert.ok(data.items.find(i => i.kind === 'prompts' && i.name === 'review'));
  const item = data.items.find(i => i.kind === 'packages');
  assert.ok(item);
  result = await helper(cwd, agentDir, { action: 'remove', scope: 'user', id: item.id, revision: data.revision });
  assert.equal(result.error, undefined);
  assert.ok(fs.existsSync(path.join(pkg, 'prompts/review.md')));
  fs.writeFileSync(path.join(agentDir, 'settings.json'), '{"packages":[{"source":null}]}');
  result = await helper(cwd, agentDir, { action: 'list' });
  assert.match(result.error, /Resource discovery failed/);
  assert.ok(!JSON.stringify(result).includes('fixture-secret'));
});
