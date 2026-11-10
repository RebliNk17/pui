import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { manage, toggleResource, editConfig } from '../scripts/resources.mjs';

function fixture({ trusted = true } = {}) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'pui-resources-')));
  const agentDir = path.join(root, 'agent');
  const cwd = path.join(root, 'project');
  fs.mkdirSync(agentDir); fs.mkdirSync(cwd);
  let lastSettings;
  const calls = [];
  class Settings {
    static create(_cwd, _agentDir, options) { lastSettings = new Settings(options); return lastSettings; }
    constructor(options) {
      this.options = options;
      this.global = JSON.parse(fs.readFileSync(path.join(agentDir, 'settings.json'), 'utf8'));
      this.project = options.projectTrusted && fs.existsSync(path.join(cwd, '.pi/settings.json')) ? JSON.parse(fs.readFileSync(path.join(cwd, '.pi/settings.json'), 'utf8')) : {};
    }
    drainErrors() { return []; }
    getDefaultProjectTrust() { return 'ask'; }
    getGlobalSettings() { return structuredClone(this.global); }
    getProjectSettings() { return structuredClone(this.project); }
    applyOverrides() {}
    setSkillPaths(entries) { this.global.skills = entries; }
    setProjectSkillPaths(entries) { this.project.skills = entries; }
    setPackages(entries) { this.global.packages = entries; }
    setProjectPackages(entries) { this.project.packages = entries; }
    async flush() { fs.writeFileSync(path.join(agentDir, 'settings.json'), JSON.stringify(this.global)); }
  }
  const resourcePath = path.join(agentDir, 'skills/example/SKILL.md');
  fs.mkdirSync(path.dirname(resourcePath), { recursive: true });
  fs.writeFileSync(resourcePath, '---\nname: example\ndescription: Fixture skill\n---\nFixture');
  fs.writeFileSync(path.join(agentDir, 'settings.json'), JSON.stringify({ other: 42, skills: [resourcePath] }));
  class Manager {
    constructor({ settingsManager }) { this.settings = settingsManager; }
    async resolve(onMissing) {
      assert.equal(await onMissing('npm:missing'), 'skip');
      calls.push('resolve');
      return { skills: [{ path: resourcePath, enabled: !(this.settings.global.skills ?? []).some(p => p.startsWith('-')), metadata: { scope: 'user', origin: 'top-level', source: 'local' } }] };
    }
    getInstalledPath() { return undefined; }
    async installAndPersist(source, options) { calls.push(['install', source, options]); }
    async removeAndPersist(source, options) { calls.push(['remove', source, options]); }
  }
  const sdk = { SettingsManager: Settings, ProjectTrustStore: class { get() { return trusted; } }, DefaultPackageManager: Manager, parseFrontmatter: () => ({ frontmatter: { name: 'example', description: 'Fixture skill' } }) };
  const env = { sdk, agentDir, cwd, validateMcp: (name, value) => /^[\w-]+$/.test(name) && value && (value.command || value.url) ? value : 'Invalid server' };
  const list = () => manage({ action: 'list' }, env);
  const apply = async request => { const catalog = await list(); return manage({ revision: catalog.revision, scope: 'user', ...request }, env); };
  return { root, agentDir, cwd, resourcePath, env, calls, list, apply, settings: () => lastSettings };
}

test('discovery skips missing installs and never returns MCP credentials/commands/URLs', async () => {
  const f = fixture();
  fs.writeFileSync(path.join(f.agentDir, 'mcp.json'), JSON.stringify({ unrelated: true, mcpServers: { docs: { url: 'https://fixture.invalid/mcp?secret=fixture-token', headers: { Authorization: 'Bearer fixture-token' } }, local: { command: 'fixture-command', args: ['fixture-token'] } } }));
  const catalog = await f.list();
  assert.equal(catalog.items.filter(i => i.kind === 'mcp').length, 2);
  assert.equal(catalog.items.find(i => i.kind === 'skills').description, 'Fixture skill');
  assert.ok(!JSON.stringify(catalog).includes('fixture-token'));
  assert.ok(!JSON.stringify(catalog).includes('fixture-command'));
  assert.ok(!JSON.stringify(catalog).includes('https://fixture.invalid'));
  assert.deepEqual(f.calls, ['resolve', 'resolve']);
});

test('stale confirmations fail before any mutation', async () => {
  const f = fixture();
  const catalog = await f.list();
  fs.writeFileSync(path.join(f.agentDir, 'settings.json'), '{"other":43}');
  await assert.rejects(manage({ action: 'remove', id: catalog.items[0].id, scope: 'user', revision: catalog.revision }, f.env), /Configuration changed/);
  assert.equal(fs.readFileSync(f.resourcePath, 'utf8').includes('Fixture'), true);
  assert.deepEqual(f.calls, ['resolve', 'resolve', 'resolve', 'resolve']);
});

test('toggle preserves explicit discovery paths, unrelated settings and source files', async () => {
  const f = fixture();
  const item = (await f.list()).items[0];
  await f.apply({ action: 'toggle', id: item.id, enabled: false });
  let config = JSON.parse(fs.readFileSync(path.join(f.agentDir, 'settings.json')));
  assert.equal(config.other, 42);
  assert.ok(config.skills.includes(f.resourcePath));
  assert.ok(config.skills.includes('-skills/example/SKILL.md'));
  await f.apply({ action: 'toggle', id: item.id, enabled: true });
  config = JSON.parse(fs.readFileSync(path.join(f.agentDir, 'settings.json')));
  assert.ok(config.skills.includes('+skills/example/SKILL.md'));
  assert.ok(!config.skills.includes('-skills/example/SKILL.md'));
  assert.ok(fs.existsSync(f.resourcePath));
});

test('package filters retain manifest selection and other types', () => {
  const f = fixture();
  const settings = { packages: [{ source: 'npm:example', skills: ['skills/*'], extensions: [] }], getGlobalSettings() { return { packages: this.packages }; }, setPackages(packages) { this.packages = packages; } };
  const item = { kind: 'skills', scope: 'user', path: '/package/skills/example/SKILL.md', metadata: { origin: 'package', source: 'npm:example', baseDir: '/package' } };
  toggleResource(settings, item, false, f.agentDir, f.cwd);
  assert.deepEqual(settings.packages[0].skills, ['skills/*', '-skills/example/SKILL.md']);
  assert.deepEqual(settings.packages[0].extensions, []);
  toggleResource(settings, item, true, f.agentDir, f.cwd);
  assert.deepEqual(settings.packages[0].skills, ['skills/*', '+skills/example/SKILL.md']);
});

test('untrusted project scope cannot be modified or shown', async () => {
  const f = fixture({ trusted: false });
  fs.mkdirSync(path.join(f.cwd, '.pi'));
  fs.writeFileSync(path.join(f.cwd, '.pi/mcp.json'), '{"mcpServers":{"project":{"url":"https://private.invalid"}}}');
  const catalog = await f.list();
  assert.equal(catalog.trusted, false);
  assert.ok(!catalog.items.some(i => i.scope === 'project'));
  await assert.rejects(f.apply({ action: 'install', scope: 'project', source: 'npm:example' }), /not trusted/);
  assert.ok(!f.calls.some(c => Array.isArray(c)));
});

test('MCP edits merge supplied fields, preserve secrets/unrelated keys, and reject duplicate adds', async () => {
  const f = fixture();
  const file = path.join(f.agentDir, 'mcp.json');
  fs.writeFileSync(file, JSON.stringify({ unrelated: 42, mcpServers: { docs: { url: 'https://example.invalid/mcp', headers: { Authorization: '${FIXTURE_TOKEN}' }, enabled: true } } }));
  const item = (await f.list()).items.find(i => i.kind === 'mcp');
  await f.apply({ action: 'mcp-edit', id: item.id, definition: '{"enabled":false,"description":"Updated"}' });
  let config = JSON.parse(fs.readFileSync(file));
  assert.equal(config.unrelated, 42);
  assert.deepEqual(config.mcpServers.docs.headers, { Authorization: '${FIXTURE_TOKEN}' });
  assert.equal(config.mcpServers.docs.enabled, false);
  await assert.rejects(f.apply({ action: 'mcp-add', name: 'docs', definition: '{"url":"https://replace.invalid"}' }), /already exists/);
  await f.apply({ action: 'remove', id: item.id });
  config = JSON.parse(fs.readFileSync(file));
  assert.deepEqual(config, { unrelated: 42, mcpServers: {} });
});

test('project MCP override patches preserve global credentials and cannot inject extra fields', async () => {
  const f = fixture();
  fs.writeFileSync(path.join(f.agentDir, 'mcp.json'), '{"mcpServers":{"docs":{"url":"https://example.invalid","headers":{"Authorization":"${TOKEN}"}}}}');
  fs.mkdirSync(path.join(f.cwd, '.pi'));
  const file = path.join(f.cwd, '.pi/mcp.json');
  fs.writeFileSync(file, '{"mcpServers":{"docs":{"enabled":false}}}');
  const item = (await f.list()).items.find(i => i.kind === 'mcp' && i.scope === 'project');
  await f.apply({ action: 'mcp-edit', scope: 'project', id: item.id, definition: '{"enabled":true,"exposure":"hidden"}' });
  assert.deepEqual(JSON.parse(fs.readFileSync(file)).mcpServers.docs, { enabled: true, exposure: 'hidden' });
  await assert.rejects(f.apply({ action: 'mcp-edit', scope: 'project', id: item.id, definition: '{"headers":{"Authorization":"other"}}' }), /Project overrides can only/);
  assert.ok(!fs.readFileSync(file, 'utf8').includes('TOKEN'));
});

test('configuration symlinks are readable but never overwritten by resource mutations', async () => {
  const f = fixture();
  const real = path.join(f.root, 'real-mcp.json');
  const file = path.join(f.agentDir, 'mcp.json');
  fs.writeFileSync(real, '{"mcpServers":{"docs":{"url":"https://example.invalid"}}}');
  fs.symlinkSync(real, file);
  const item = (await f.list()).items.find(i => i.kind === 'mcp');
  await assert.rejects(f.apply({ action: 'toggle', id: item.id, enabled: false }), /symlink/);
  assert.equal(JSON.parse(fs.readFileSync(real)).mcpServers.docs.enabled, undefined);
});

test('invalid configuration is never silently replaced and Pui locking refuses concurrent edits', async () => {
  const f = fixture();
  const file = path.join(f.agentDir, 'mcp.json');
  fs.writeFileSync(file, '{invalid');
  assert.throws(() => editConfig(file, c => { c.mcpServers = {}; }), /Cannot read configuration/);
  assert.equal(fs.readFileSync(file, 'utf8'), '{invalid');
  fs.mkdirSync(`${file}.pui-lock`);
  assert.throws(() => editConfig(file, () => {}), /being edited/);
});

test('register adds a discovery path; removal excludes rather than deletes local resources', async () => {
  const f = fixture();
  await f.apply({ action: 'register', kind: 'skills', source: f.resourcePath });
  const config = JSON.parse(fs.readFileSync(path.join(f.agentDir, 'settings.json')));
  assert.ok(config.skills.includes(f.resourcePath));
  const item = (await f.list()).items.find(i => i.kind === 'skills');
  await f.apply({ action: 'remove', id: item.id });
  assert.ok(fs.existsSync(f.resourcePath));
});

test('install and uninstall use scope-specific Pi APIs, never shell interpolation', async () => {
  const f = fixture();
  await f.apply({ action: 'install', source: 'npm:example@1.0.0' });
  assert.deepEqual(f.calls.at(-1), ['install', 'npm:example@1.0.0', { local: false }]);
  const file = path.join(f.agentDir, 'settings.json');
  fs.writeFileSync(file, '{"packages":["npm:example@1.0.0"]}');
  const item = (await f.list()).items.find(i => i.kind === 'packages');
  await f.apply({ action: 'remove', id: item.id });
  assert.deepEqual(f.calls.at(-1), ['remove', 'npm:example@1.0.0', { local: false }]);
});

// Optional native SDK fixture: private temporary config only, no user's settings,
// providers, extension execution, MCP connection, network or remote installs.
let sdkPath = process.env.PUI_PI_SDK;
if (!sdkPath) {
  const agentDir = process.env.PI_CODING_AGENT_DIR ?? path.join(os.homedir(), '.pi/agent');
  try { sdkPath = path.join(agentDir, 'install/releases', fs.readFileSync(path.join(agentDir, 'install/current-version'), 'utf8').trim(), 'node_modules/@earendil-works/pi-coding-agent/dist/index.js'); } catch {}
}
test('compatible Pi SDK discovers, toggles, and registers a real local skill without loading extensions', { skip: !sdkPath || !fs.existsSync(sdkPath) }, async () => {
  const f = fixture();
  const sdk = await import(pathToFileURL(sdkPath).href);
  const { validateMcpServerConfig } = await import(pathToFileURL(path.join(path.dirname(sdkPath), 'core/mcp-servers.js')).href);
  const env = { sdk, validateMcp: validateMcpServerConfig, cwd: f.cwd, agentDir: f.agentDir };
  const extension = path.join(f.agentDir, 'extensions/never-run.ts');
  fs.mkdirSync(path.dirname(extension));
  fs.writeFileSync(extension, `throw new Error('Resource discovery must not execute extension code');`);
  let catalog = await manage({ action: 'list' }, env);
  let item = catalog.items.find(i => i.kind === 'skills' && i.name === 'example');
  assert.ok(item);
  await manage({ action: 'toggle', id: item.id, scope: 'user', enabled: false, revision: catalog.revision }, env);
  catalog = await manage({ action: 'list' }, env);
  item = catalog.items.find(i => i.id === item.id);
  assert.ok(item && !item.enabled);
  await manage({ action: 'toggle', id: item.id, scope: 'user', enabled: true, revision: catalog.revision }, env);
  const external = path.join(f.root, 'outside/SKILL.md');
  fs.mkdirSync(path.dirname(external));
  fs.writeFileSync(external, '---\nname: external\ndescription: External fixture\n---\nFixture');
  catalog = await manage({ action: 'list' }, env);
  await manage({ action: 'register', kind: 'skills', source: external, scope: 'user', revision: catalog.revision }, env);
  catalog = await manage({ action: 'list' }, env);
  assert.ok(catalog.items.find(i => i.name === 'external' && i.enabled));
});
