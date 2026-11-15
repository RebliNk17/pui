import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { manage } from '../scripts/resources.mjs';
let sdkPath = process.env.PUI_PI_SDK;
if (!sdkPath) { const agent = process.env.PI_CODING_AGENT_DIR ?? path.join(os.homedir(), '.pi/agent'); try { sdkPath = path.join(agent, 'install/releases', fs.readFileSync(path.join(agent, 'install/current-version'), 'utf8').trim(), 'node_modules/@earendil-works/pi-coding-agent/dist/index.js'); } catch {} }
const available = sdkPath && fs.existsSync(sdkPath);
const engineering = fileURLToPath(new URL('../assets/pui-engineering/', import.meta.url));
const engineeringNames = ['clarify', 'define', 'tdd', 'debug', 'code-review'];

test('Pui Engineering ships only original explicit-invocation skill files, with no executable package content', () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(engineering, 'package.json')));
  assert.equal(manifest.name, 'pui-engineering');
  assert.deepEqual(manifest.pi, { skills: ['skills'], extensions: [], prompts: [], themes: [] });
  assert.equal(manifest.dependencies, undefined);
  assert.equal(manifest.scripts, undefined);
  assert.deepEqual(fs.readdirSync(path.join(engineering, 'skills')).sort(), [...engineeringNames].sort());
  for (const name of engineeringNames) {
    const dir = path.join(engineering, 'skills', name);
    assert.deepEqual(fs.readdirSync(dir), ['SKILL.md']);
    const content = fs.readFileSync(path.join(dir, 'SKILL.md'), 'utf8');
    assert.ok(content.startsWith(`---\nname: ${name}\ndescription: `));
    assert.match(content, /disable-model-invocation: true/);
    assert.ok(Buffer.byteLength(content) < 65536);
    assert.doesNotMatch(content, /Call the Skill tool|mattpocock|aihero\.dev/);
  }
});
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

test('Pui Engineering previews do not enable skills; reviewed registration, explicit loading and disable use real Pi', { skip: !available }, async () => {
  const f = await fixture();
  const sdk = f.env.sdk;
  const parsed = sdk.loadSkillsFromDir({ dir: path.join(engineering, 'skills'), source: 'pui' });
  assert.deepEqual(parsed.diagnostics, []);
  assert.deepEqual(parsed.skills.map(s => s.name).sort(), [...engineeringNames].sort());
  assert.ok(parsed.skills.every(s => s.disableModelInvocation));
  assert.equal(sdk.formatSkillsForPrompt(parsed.skills), '');
  let catalog = await f.list();
  assert.ok(!catalog.items.some(i => engineeringNames.includes(i.name)));
  const operations = engineeringNames.map(name => {
    const source = path.join(engineering, 'skills', name, 'SKILL.md');
    return { action: 'register', kind: 'skills', scope: 'project', source, contentHash: createHash('sha256').update(fs.readFileSync(source)).digest('hex') };
  });
  const review = await f.preview(catalog, operations);
  assert.equal(review.steps.length, 5);
  assert.ok(!fs.existsSync(path.join(f.cwd, '.pi/settings.json')));
  assert.equal((await f.apply(catalog, operations, review)).complete, true);
  catalog = await f.list();
  assert.equal(catalog.items.filter(i => engineeringNames.includes(i.name) && i.enabled).length, 5);
  const load = async () => {
    const settingsManager = sdk.SettingsManager.create(f.cwd, f.agentDir, { projectTrusted: true });
    const loader = new sdk.DefaultResourceLoader({ cwd: f.cwd, agentDir: f.agentDir, settingsManager, noExtensions: true, noContextFiles: true, noThemes: true, noPromptTemplates: true });
    await loader.reload();
    return loader.getSkills();
  };
  const loaded = await load();
  assert.deepEqual(loaded.diagnostics, []);
  assert.equal(loaded.skills.filter(s => engineeringNames.includes(s.name)).length, 5);
  const tdd = catalog.items.find(i => i.name === 'tdd');
  const off = [{ action: 'select', id: tdd.id, scope: 'project', state: 'off' }];
  assert.equal((await f.apply(catalog, off, await f.preview(catalog, off))).complete, true);
  assert.equal((await f.list()).items.find(i => i.name === 'tdd').enabled, false);
  assert.ok(!(await load()).skills.some(s => s.name === 'tdd'));
  assert.ok(fs.existsSync(tdd.path));
});

test('bundled content checks reject changed files, invalid hashes and untrusted scope without activation', { skip: !available }, async () => {
  const f = await fixture();
  const source = path.join(f.root, 'tdd', 'SKILL.md');
  fs.mkdirSync(path.dirname(source));
  fs.copyFileSync(path.join(engineering, 'skills/tdd/SKILL.md'), source);
  const contentHash = createHash('sha256').update(fs.readFileSync(source)).digest('hex');
  const operations = [{ action: 'register', kind: 'skills', scope: 'user', source, contentHash }];
  let catalog = await f.list();
  const review = await f.preview(catalog, operations);
  fs.appendFileSync(source, '\nChanged after review.\n');
  await assert.rejects(f.apply(catalog, operations, review), /Skill contents changed/);
  assert.equal(JSON.parse(fs.readFileSync(path.join(f.agentDir, 'settings.json'))).skills, undefined);
  operations[0].contentHash = 'invalid';
  await assert.rejects(f.preview(catalog, operations), /Invalid resource content hash/);
  fs.unlinkSync(source);
  fs.symlinkSync(path.join(engineering, 'skills/tdd/SKILL.md'), source);
  operations[0].contentHash = contentHash;
  await assert.rejects(f.preview(catalog, operations), /symlink/);
  new f.env.sdk.ProjectTrustStore(f.agentDir).set(f.cwd, false);
  catalog = await f.list();
  operations[0] = { action: 'register', kind: 'skills', scope: 'project', source: path.join(engineering, 'skills/tdd/SKILL.md'), contentHash };
  await assert.rejects(f.preview(catalog, operations), /not trusted/);
  assert.ok(!fs.existsSync(path.join(f.cwd, '.pi/settings.json')));
});

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
