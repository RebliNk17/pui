// Resource management runs in a short-lived helper, never in the agent process.
// Discovery uses the package resolver, not the resource loader: no extension code,
// MCP connections, missing-package installs, or model/provider requests.
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash, randomUUID } from 'node:crypto';

export const TYPES = ['skills', 'mcp', 'extensions', 'prompts', 'themes', 'packages'];
const PATH_SETTERS = { skills: 'SkillPaths', extensions: 'ExtensionPaths', prompts: 'PromptTemplatePaths', themes: 'ThemePaths' };
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const hash = value => createHash('sha256').update(value).digest('hex');
const sourceOf = pkg => typeof pkg === 'string' ? pkg : pkg.source;
const strip = entry => /^[!+-]/.test(entry) ? entry.slice(1) : entry;
class ResourceError extends Error {}
const fail = message => { throw new ResourceError(message); };

function readConfig(file) {
  try {
    if (!fs.existsSync(file)) return {};
    if (fs.statSync(file).size > 1024 * 1024) fail('Configuration file exceeds 1 MiB.');
    const parsed = JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, ''));
    if (!object(parsed)) fail('Invalid configuration.');
    return parsed;
  } catch { fail('Cannot read configuration. Fix its JSON or file permissions before retrying.'); }
}

function assertWritableConfig(file) {
  for (let parent = file; ; parent = path.dirname(parent)) {
    try {
      if (fs.lstatSync(parent).isSymbolicLink()) fail('Refusing to write through a configuration symlink.');
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    if (path.dirname(parent) === parent) break;
  }
}
// Do not truncate/replace invalid files or follow config symlinks when writing.
export function editConfig(file, edit) {
  assertWritableConfig(file);
  fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
  const lock = `${file}.pui-lock`;
  try { fs.mkdirSync(lock, { mode: 0o700 }); }
  catch { fail('Configuration is being edited. Retry after the other operation finishes.'); }
  const temp = `${file}.${randomUUID()}.tmp`;
  try {
    const before = fs.existsSync(file) ? fs.readFileSync(file) : undefined;
    const config = readConfig(file);
    edit(config);
    const current = fs.existsSync(file) ? fs.readFileSync(file) : undefined;
    if (before?.toString() !== current?.toString()) fail('Configuration changed. Refresh before retrying.');
    fs.writeFileSync(temp, `${JSON.stringify(config, null, 2)}\n`, { mode: 0o600, flag: 'wx' });
    fs.renameSync(temp, file);
  } finally {
    if (fs.existsSync(temp)) fs.unlinkSync(temp);
    fs.rmdirSync(lock);
  }
}

function revision(agentDir, cwd, trusted) {
  const files = [path.join(agentDir, 'settings.json'), path.join(agentDir, 'mcp.json')];
  if (trusted) files.push(path.join(cwd, '.pi/settings.json'), path.join(cwd, '.pi/mcp.json'));
  return hash(files.map(file => `${file}\0${fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : ''}`).join('\0'));
}
function settingsFor(settings, scope) {
  return scope === 'project' ? settings.getProjectSettings() : settings.getGlobalSettings();
}
function setPaths(settings, type, scope, entries) {
  const setter = `set${scope === 'project' ? 'Project' : ''}${PATH_SETTERS[type]}`;
  settings[setter](entries);
}
function setPackages(settings, scope, packages) {
  settings[scope === 'project' ? 'setProjectPackages' : 'setPackages'](packages);
}
function resourcePattern(item, agentDir, cwd) {
  if (item.path.startsWith('builtin:')) return item.path;
  const base = item.metadata.baseDir ?? (item.scope === 'project' ? path.join(cwd, '.pi') : agentDir);
  return path.relative(base, item.path).split(path.sep).join('/');
}
export function toggleResource(settings, item, enabled, agentDir, cwd) {
  const config = settingsFor(settings, item.scope);
  const pattern = resourcePattern(item, agentDir, cwd);
  // Plain entries discover explicitly registered paths; keep them so a disabled
  // resource remains discoverable (and can be enabled again).
  const update = entries => [...(entries ?? []).filter(p => !/^[!+-]/.test(p) || strip(p) !== pattern), `${enabled ? '+' : '-'}${pattern}`];
  if (item.metadata.origin === 'package') {
    const packages = structuredClone(config.packages ?? []);
    const index = packages.findIndex(pkg => sourceOf(pkg) === item.metadata.source);
    if (index < 0) fail('Package configuration changed. Refresh before retrying.');
    const pkg = typeof packages[index] === 'string' ? { source: packages[index] } : packages[index];
    pkg[item.kind] = update(pkg[item.kind]);
    packages[index] = pkg;
    setPackages(settings, item.scope, packages);
  } else {
    setPaths(settings, item.kind, item.scope, update(config[item.kind]));
  }
}
function details(sdk, kind, file) {
  if (file.startsWith('builtin:')) return { name: file, description: 'Built-in Pi extension' };
  let name = path.basename(file, path.extname(file));
  let description = '';
  if (kind === 'skills' || kind === 'prompts') {
    const target = fs.existsSync(file) && fs.statSync(file).isDirectory() ? path.join(file, 'SKILL.md') : file;
    if (path.basename(target) === 'SKILL.md') name = path.basename(path.dirname(target));
    try {
      if (fs.statSync(target).size <= 65536) {
        const { frontmatter } = sdk.parseFrontmatter(fs.readFileSync(target, 'utf8'));
        if (kind === 'skills' && typeof frontmatter.name === 'string') name = frontmatter.name;
        if (typeof frontmatter.description === 'string') description = frontmatter.description.slice(0, 512);
      }
    } catch { /* A broken resource stays visible by its path. */ }
  }
  return { name, description };
}

export async function manage(request, { sdk, agentDir, cwd, validateMcp }) {
  cwd = fs.realpathSync(cwd);
  if (fs.existsSync(agentDir)) agentDir = fs.realpathSync(agentDir);
  if (!TYPES.includes(request.kind ?? 'skills')) fail('Unknown resource type.');
  // Validate before SettingsManager can migrate or repair a malformed config.
  readConfig(path.join(agentDir, 'settings.json'));
  const savedTrust = new sdk.ProjectTrustStore(agentDir).get(cwd);
  const userSettings = sdk.SettingsManager.create(cwd, agentDir, { projectTrusted: false });
  const trusted = savedTrust ?? userSettings.getDefaultProjectTrust() === 'always';
  if (trusted) readConfig(path.join(cwd, '.pi/settings.json'));
  const settings = sdk.SettingsManager.create(cwd, agentDir, { projectTrusted: trusted });
  if (settings.drainErrors().length) fail('Pi could not load settings. Fix the configuration before retrying.');
  const manager = new sdk.DefaultPackageManager({ cwd, agentDir, settingsManager: settings,
    builtinExtensions: ['mcp', 'llama.cpp', 'codemode', 'tool-search'] });
  // npm root lookup must not execute a project-provided npmCommand while browsing.
  const npmCommand = settings.getSettings?.().npmCommand;
  settings.applyOverrides({ npmCommand: ['npm'] });
  const resources = await manager.resolve(async () => 'skip');
  const items = [];
  const warnings = [];
  for (const kind of Object.keys(PATH_SETTERS)) {
    for (const resource of resources[kind] ?? []) {
      const scope = resource.metadata.scope;
      if (!['user', 'project'].includes(scope)) continue;
      items.push({ ...resource, kind, scope, ...details(sdk, kind, resource.path),
        source: resource.metadata.source,
        config: path.join(scope === 'project' ? path.join(cwd, '.pi') : agentDir, 'settings.json'),
        removable: !resource.path.startsWith('builtin:') });
    }
  }
  for (const scope of trusted ? ['user', 'project'] : ['user']) {
    for (const pkg of settingsFor(settings, scope).packages ?? []) {
      const source = sourceOf(pkg);
      const installed = manager.getInstalledPath(source, scope);
      items.push({ kind: 'packages', scope, name: source, source, path: installed ?? '',
        description: installed && fs.existsSync(installed) ? 'Installed package' : 'Not installed or unavailable',
        enabled: true, removable: true, config: path.join(scope === 'project' ? path.join(cwd, '.pi') : agentDir, 'settings.json') });
    }
    const file = path.join(scope === 'project' ? path.join(cwd, '.pi') : agentDir, 'mcp.json');
    try {
      const config = readConfig(file);
      if (config.mcpServers !== undefined && !object(config.mcpServers)) fail('Invalid MCP configuration.');
      for (const [name, server] of Object.entries(config.mcpServers ?? {})) {
        const valid = validateMcp(name, server);
        const override = scope === 'project' && object(server) && !server.command && !server.url && !server.type;
        items.push({ kind: 'mcp', scope, name, source: file, path: file, config: file,
          description: override ? 'Project override of a personal server' : typeof valid === 'string' ? 'Invalid server definition — edit to repair' : server.url ? 'Streamable HTTP server' : 'Local stdio server',
          enabled: object(server) && server.enabled !== false, removable: true });
      }
    } catch { warnings.push(`${scope === 'user' ? 'Personal' : 'Project'} MCP configuration cannot be read. Fix its JSON before editing.`); }
  }
  for (const item of items) item.id = hash(`${item.kind}\0${item.scope}\0${item.path}\0${item.name}`);
  const currentRevision = revision(agentDir, cwd, trusted);
  if (request.action && request.action !== 'list') {
    if (request.revision !== currentRevision) fail('Configuration changed. Refresh and review the action again.');
    const scope = request.scope;
    if (!['user', 'project'].includes(scope)) fail('Choose a valid configuration scope.');
    if (scope === 'project' && !trusted) fail('Project resources are not trusted. Use Pi /trust first.');
    const scopeDir = scope === 'project' ? path.join(cwd, '.pi') : agentDir;
    assertWritableConfig(path.join(scopeDir, 'settings.json'));
    assertWritableConfig(path.join(scopeDir, 'mcp.json'));
    const item = items.find(item => item.id === request.id && item.scope === scope);
    const action = request.action;
    if (['toggle', 'remove', 'mcp-edit'].includes(action) && !item) fail('Resource no longer exists. Refresh before retrying.');
    // Restore the configured package manager only for the explicitly reviewed
    // mutation. Installed-path lookup during browsing also uses plain npm.
    settings.applyOverrides({ npmCommand: npmCommand ?? ['npm'] });
    if (action === 'toggle') {
      if (item.kind === 'mcp') editConfig(item.config, config => { config.mcpServers[item.name].enabled = !!request.enabled; });
      else if (PATH_SETTERS[item.kind]) toggleResource(settings, item, !!request.enabled, agentDir, cwd);
      else fail('This resource does not support toggling.');
    } else if (action === 'install') {
      const source = request.source?.trim();
      if (!source || source.length > 4096 || source.startsWith('-')) fail('Enter a package source.');
      // Explicitly confirmed operation: installers/dependencies can execute code.
      await manager.installAndPersist(source, { local: scope === 'project' });
    } else if (action === 'register') {
      if (!PATH_SETTERS[request.kind]) fail('Choose a local resource type.');
      let target = request.source?.trim();
      if (!target || target.length > 4096) fail('Enter a local resource path.');
      if (target.startsWith('~/')) target = path.join(process.env.HOME ?? '', target.slice(2));
      target = path.resolve(cwd, target);
      if (!fs.existsSync(target)) fail('Local resource path does not exist.');
      const current = settingsFor(settings, scope)[request.kind] ?? [];
      setPaths(settings, request.kind, scope, [...current.filter(p => strip(p) !== target), target, `+${target}`]);
    } else if (action === 'mcp-add' || action === 'mcp-edit') {
      const name = item?.name ?? request.name?.trim();
      if (!name || !/^[a-zA-Z0-9_-]+$/.test(name) || ['__proto__', 'constructor', 'prototype'].includes(name)) fail('Server names can contain letters, digits, underscores and hyphens; reserved object names are not supported.');
      let patch;
      try { patch = JSON.parse(request.definition); } catch { fail('Server definition must be valid JSON.'); }
      if (!object(patch)) fail('Server definition must be a JSON object.');
      const file = path.join(scope === 'project' ? path.join(cwd, '.pi') : agentDir, 'mcp.json');
      editConfig(file, config => {
        if (config.mcpServers !== undefined && !object(config.mcpServers)) fail('Invalid mcpServers object.');
        config.mcpServers ??= {};
        if (action === 'mcp-add' && Object.hasOwn(config.mcpServers, name)) fail('Server already exists. Use Edit definition instead.');
        const next = { ...(item ? config.mcpServers[name] : {}), ...patch };
        const override = scope === 'project' && !next.command && !next.url && !next.type;
        const base = override ? readConfig(path.join(agentDir, 'mcp.json')).mcpServers?.[name] : undefined;
        if (override && (!object(base) || Object.keys(next).some(key => !['enabled', 'exposure', 'toolExposure'].includes(key)))) fail('Invalid MCP definition. Project overrides can only set enabled, exposure and toolExposure.');
        if (typeof validateMcp(name, { ...base, ...next }) === 'string') fail('Invalid MCP definition. Check command/args or URL, transport and exposure.');
        if (scope === 'project' && next.auth) fail('Provider auth is only supported in personal MCP configuration.');
        const clash = Object.keys(config.mcpServers).some(other => other !== name && other.replaceAll('-', '_') === name.replaceAll('-', '_'));
        if (clash) fail('Another server uses the same normalized name.');
        config.mcpServers[name] = next;
      });
    } else if (action === 'remove') {
      if (!item.removable) fail('Built-in resources cannot be removed. Disable them instead.');
      if (item.kind === 'packages') await manager.removeAndPersist(item.source, { local: scope === 'project' });
      else if (item.kind === 'mcp') editConfig(item.config, config => { delete config.mcpServers[item.name]; });
      else {
        // Never delete local source files. Exclude autodiscovered resources and
        // unregister exact explicit paths, matching Pi's config filter semantics.
        toggleResource(settings, item, false, agentDir, cwd);
      }
    } else fail('Unknown resource action.');
    await settings.flush();
    if (settings.drainErrors().length) fail('Pi could not save configuration. Refresh to inspect its current state.');
    return { changed: true };
  }
  if (!trusted) warnings.push('Project resources are hidden until trusted in Pi. Personal resources are still manageable.');
  if (items.length > 2000) warnings.push('Showing the first 2,000 resources. Narrow your configuration to see more.');
  return { revision: currentRevision, trusted, items: items.slice(0, 2000).map(({ metadata, ...item }) => item), warnings,
    userConfig: path.join(agentDir, 'settings.json'), projectConfig: path.join(cwd, '.pi/settings.json') };
}

async function main() {
  let emit = async record => { process.stdout.write(JSON.stringify(record)); };
  let mutated = false;
  try {
    const input = fs.readFileSync(0, 'utf8');
    if (input.length > 131072) fail('Resource request is too large.');
    const request = JSON.parse(input);
    if (!object(request)) fail('Invalid resource request.');
    mutated = request.action && request.action !== 'list';
    const sdkPath = path.resolve(process.argv[1]);
    const { takeOverStdout, writeRawStdout, flushRawStdout } = await import(pathToFileURL(path.join(path.dirname(sdkPath), 'core/output-guard.js')).href);
    takeOverStdout();
    emit = async record => { writeRawStdout(JSON.stringify(record)); await flushRawStdout(); };
    const sdk = await import(pathToFileURL(sdkPath).href);
    const { validateMcpServerConfig } = await import(pathToFileURL(path.join(path.dirname(sdkPath), 'core/mcp-servers.js')).href);
    const data = await manage(request, { sdk, agentDir: sdk.getAgentDir(), cwd: process.cwd(), validateMcp: validateMcpServerConfig });
    await emit({ data });
  } catch (error) {
    // Never forward installer stdout/stderr, config values, or arbitrary SDK
    // exceptions (which may contain tokens or command arguments) to the UI.
    await emit({ error: error instanceof ResourceError ? error.message : mutated ? 'Resource operation failed. It may have partially completed; refresh before retrying. Check the Pi CLI for details.' : 'Resource discovery failed. Use a compatible Pi SDK (PUI_PI_SDK) and check configuration.' });
  }
}
if (process.env.PUI_RESOURCES_HELPER === '1') await main();
