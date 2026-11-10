import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { manage } from '../scripts/resources.mjs';
let sdkPath = process.env.PUI_PI_SDK;
if (!sdkPath) { const agent = process.env.PI_CODING_AGENT_DIR ?? path.join(os.homedir(), '.pi/agent'); try { sdkPath = path.join(agent, 'install/releases', fs.readFileSync(path.join(agent, 'install/current-version'), 'utf8').trim(), 'node_modules/@earendil-works/pi-coding-agent/dist/index.js'); } catch {} }
const available = sdkPath && fs.existsSync(sdkPath);
async function fixture() {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'pui-resource-manager-')));
  const agentDir = path.join(root, 'agent'), cwd = path.join(root, 'project');
  fs.mkdirSync(path.join(agentDir, 'skills/example'), { recursive: true }); fs.mkdirSync(path.join(cwd, '.pi'), { recursive: true });
  fs.writeFileSync(path.join(agentDir, 'skills/example/SKILL.md'), '---\nname: example\ndescription: Fixture skill\n---\nRead the project context.');
  fs.writeFileSync(path.join(agentDir, 'settings.json'), '{"other":42}');
  const sdk = await import(pathToFileURL(sdkPath).href);
  const { validateMcpServerConfig } = await import(pathToFileURL(path.join(path.dirname(sdkPath), 'core/mcp-servers.js')).href);
  new sdk.ProjectTrustStore(agentDir).set(cwd, true);
  const env = { sdk, validateMcp: validateMcpServerConfig, agentDir, cwd };
  const list = () => manage({ action: 'list' }, env);
  const preview = (catalog, operations) => manage({ action: 'preview', revision: catalog.revision, operations }, env);
  const apply = (catalog, operations, review) => manage({ action: 'apply', revision: catalog.revision, operations, token: review.token }, env);
  return { root, agentDir, cwd, env, list, preview, apply };
}

test('native project inheritance, personal defaults, review tokens and reset are real Pi filters', { skip: !available }, async () => {
  const f = await fixture(); let catalog = await f.list(); let item = catalog.items.find(r => r.name === 'example');
  assert.equal(item.inherited, true); assert.equal(item.personalEnabled, true); assert.equal(item.projectState, 'inherit');
  let operations = [{ action: 'select', id: item.id, scope: 'project', state: 'off' }];
  let review = await f.preview(catalog, operations);
  assert.ok(review.steps[0].after.includes('-')); assert.ok(!fs.existsSync(path.join(f.cwd, '.pi/settings.json')));
  await assert.rejects(f.apply(catalog, operations, { token: 'not-reviewed' }), /Review is out of date/);
  assert.equal((await f.apply(catalog, operations, review)).complete, true);
  catalog = await f.list(); item = catalog.items.find(r => r.path === item.path);
  assert.equal(item.enabled, false); assert.equal(item.personalEnabled, true); assert.equal(item.projectState, 'off');
  operations = [{ action: 'select', id: item.id, scope: 'project', state: 'inherit' }];
  review = await f.preview(catalog, operations); assert.equal((await f.apply(catalog, operations, review)).complete, true);
  catalog = await f.list(); item = catalog.items.find(r => r.path === item.path); assert.equal(item.enabled, true); assert.equal(item.projectState, 'inherit');
  operations = [{ action: 'select', id: item.id, scope: 'user', state: 'off' }];
  review = await f.preview(catalog, operations); assert.equal((await f.apply(catalog, operations, review)).complete, true);
  catalog = await f.list(); item = catalog.items.find(r => r.path === item.path); assert.equal(item.personalEnabled, false); assert.equal(item.enabled, false);
  assert.equal(JSON.parse(fs.readFileSync(path.join(f.agentDir, 'settings.json'))).other, 42);
});

test('MCP review redacts secrets, preserves auth and supports project inherit without connecting', { skip: !available }, async () => {
  const f = await fixture(); const file = path.join(f.agentDir, 'mcp.json');
  fs.writeFileSync(file, '{"other":7,"mcpServers":{"docs":{"url":"https://example.invalid/?token=fixture-secret","headers":{"Authorization":"fixture-secret"},"env":{"TOKEN":"fixture-secret"}}}}');
  // Use valid HTTP config; env is ignored by Pi validation where supported.
  let catalog = await f.list(); let item = catalog.items.find(r => r.kind === 'mcp'); assert.ok(item);
  let operations = [{ action: 'select', id: item.id, scope: 'project', state: 'off' }];
  let review = await f.preview(catalog, operations); assert.ok(!JSON.stringify(review).includes('fixture-secret'));
  assert.equal((await f.apply(catalog, operations, review)).complete, true);
  catalog = await f.list(); item = catalog.items.find(r => r.kind === 'mcp'); assert.equal(item.enabled, false); assert.equal(item.personalEnabled, true);
  operations = [{ action: 'select', id: item.id, scope: 'project', state: 'inherit' }]; review = await f.preview(catalog, operations); await f.apply(catalog, operations, review);
  catalog = await f.list(); item = catalog.items.find(r => r.kind === 'mcp'); assert.equal(item.enabled, true);
  operations = [{ action: 'mcp-edit', id: item.id, scope: 'user', definition: '{"exposure":"hidden"}' }]; review = await f.preview(catalog, operations);
  assert.ok(!JSON.stringify(review).includes('fixture-secret')); await f.apply(catalog, operations, review);
  const config = JSON.parse(fs.readFileSync(file)); assert.equal(config.mcpServers.docs.headers.Authorization, 'fixture-secret'); assert.equal(config.other, 7); assert.equal(config.mcpServers.docs.exposure, 'hidden');
});

test('explicit previews are bounded and reject symlinks; review rejects stale config', { skip: !available }, async () => {
  const f = await fixture(); let catalog = await f.list(); const item = catalog.items.find(r => r.name === 'example');
  const preview = await manage({ action: 'inspect', id: item.id, revision: catalog.revision }, f.env); assert.match(preview.content, /Read the project context/);
  const operations = [{ action: 'select', id: item.id, scope: 'project', state: 'off' }]; const review = await f.preview(catalog, operations);
  fs.writeFileSync(path.join(f.agentDir, 'settings.json'), '{"other":43}'); await assert.rejects(f.apply(catalog, operations, review), /Configuration changed/);
  fs.unlinkSync(item.path); const real = path.join(f.root, 'source.md'); fs.writeFileSync(real, '---\nname: example\ndescription: Fixture\n---'); fs.symlinkSync(real, item.path);
  catalog = await f.list(); await assert.rejects(manage({ action: 'inspect', id: item.id, revision: catalog.revision }, f.env), /symlink/);
});

test('sequential review reports individual results and keeps successful configuration writes', { skip: !available }, async () => {
  const f = await fixture(); const catalog = await f.list(); const item = catalog.items.find(r => r.name === 'example');
  const operations = [{ action: 'select', id: item.id, scope: 'project', state: 'off' }, { action: 'install', scope: 'user', source: '/fixture-does-not-exist/local-package' }];
  const review = await f.preview(catalog, operations);
  const result = await f.apply(catalog, operations, review);
  assert.equal(result.complete, false); assert.equal(result.results.length, 2); assert.equal(result.results[0].success, true); assert.equal(result.results[1].success, false);
  const next = await f.list(); assert.equal(next.items.find(r => r.path === item.path).enabled, false);
});

test('reviewed local package install and uninstall use Pi without deleting local source files', { skip: !available }, async () => {
  const f = await fixture(); const pkg = path.join(f.root, 'local-pkg'); fs.mkdirSync(path.join(pkg, 'prompts'), { recursive: true });
  fs.writeFileSync(path.join(pkg, 'package.json'), '{"name":"reviewed-local","pi":{"prompts":["prompts/review.md"]}}'); fs.writeFileSync(path.join(pkg, 'prompts/review.md'), 'Review local changes');
  let catalog = await f.list(); let operations = [{ action: 'install', scope: 'user', source: pkg }]; let review = await f.preview(catalog, operations); assert.equal((await f.apply(catalog, operations, review)).complete, true);
  catalog = await f.list(); const item = catalog.items.find(r => r.kind === 'packages' && r.path === pkg); assert.ok(item);
  operations = [{ action: 'remove', id: item.id, scope: 'user' }]; review = await f.preview(catalog, operations); assert.equal((await f.apply(catalog, operations, review)).complete, true); assert.ok(fs.existsSync(path.join(pkg, 'prompts/review.md')));
});

test('multiple edits to one config preserve unrelated values and every reviewed step', { skip: !available }, async () => {
  const f = await fixture(); const catalog = await f.list(); const item = catalog.items.find(r => r.name === 'example');
  const operations = [{ action: 'select', id: item.id, scope: 'project', state: 'off' }, { action: 'mcp-add', scope: 'project', name: 'docs', definition: '{"url":"https://example.invalid/mcp"}' }, { action: 'select', id: item.id, scope: 'user', state: 'off' }];
  const review = await f.preview(catalog, operations); const result = await f.apply(catalog, operations, review); assert.equal(result.complete, true); assert.equal(result.results.length, 3);
  assert.equal(JSON.parse(fs.readFileSync(path.join(f.agentDir, 'settings.json'))).other, 42);
});

test('package contents have ownership, per-resource project filters, and removal impact', { skip: !available }, async () => {
  const f = await fixture(); const pkg = path.join(f.root, 'package'); fs.mkdirSync(path.join(pkg, 'skills/example'), { recursive: true });
  fs.writeFileSync(path.join(pkg, 'package.json'), '{"name":"fixture","version":"1.2.3","pi":{"skills":["skills/example"]}}');
  fs.writeFileSync(path.join(pkg, 'skills/example/SKILL.md'), '---\nname: packaged\ndescription: Fixture packaged skill\n---');
  fs.writeFileSync(path.join(f.agentDir, 'settings.json'), JSON.stringify({ packages: [pkg] }));
  let catalog = await f.list(); let item = catalog.items.find(r => r.name === 'packaged'); const packageItem = catalog.items.find(r => r.kind === 'packages');
  assert.equal(item.owner, packageItem.owner); assert.equal(packageItem.version, '1.2.3');
  let operations = [{ action: 'select', id: item.id, scope: 'project', state: 'off' }]; let review = await f.preview(catalog, operations); await f.apply(catalog, operations, review);
  catalog = await f.list(); item = catalog.items.find(r => r.name === 'packaged'); assert.equal(item.enabled, false); assert.equal(item.personalEnabled, true);
  operations = [{ action: 'select', id: item.id, scope: 'project', state: 'inherit' }]; review = await f.preview(catalog, operations); await f.apply(catalog, operations, review);
  catalog = await f.list(); item = catalog.items.find(r => r.name === 'packaged'); assert.equal(item.enabled, true);
  operations = [{ action: 'remove', id: packageItem.id, scope: 'user' }]; review = await f.preview(catalog, operations); assert.ok(review.steps[0].affected.includes('packaged')); assert.match(review.steps[0].operation, /Uninstall/);
});
