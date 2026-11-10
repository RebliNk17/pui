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
  const personalManager = new sdk.DefaultPackageManager({ cwd, agentDir, settingsManager: userSettings,
    builtinExtensions: ['mcp', 'llama.cpp', 'codemode', 'tool-search'] });
  userSettings.applyOverrides({ npmCommand: ['npm'] });
  const personalResources = trusted ? await personalManager.resolve(async () => 'skip') : resources;
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
  enrichCatalog(items, resources, personalResources, settings, agentDir, cwd, trusted);
  for (const item of items) item.id = hash(`${item.kind}\0${item.scope}\0${item.path}\0${item.name}`);
  const currentRevision = revision(agentDir, cwd, trusted);
  const context = { items, settings, manager, agentDir, cwd, trusted, currentRevision, validateMcp, npmCommand, sdk };
  if (request.action === 'inspect') return inspectResource(request, context);
  if (request.action === 'preview' || request.action === 'apply') return batchRequest(request, context);
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
  return { revision: currentRevision, trusted, items: items.slice(0, 2000).map(({ metadata, personalMetadata, ...item }) => item), warnings,
    userConfig: path.join(agentDir, 'settings.json'), projectConfig: path.join(cwd, '.pi/settings.json') };
}

// Manager v2: provenance, bounded explicit reads, and reviewed sequential edits.
function resourceKey(kind, file) { return `${kind}\0${file}`; }
function settingsBase(scope, agentDir, cwd) { return scope === 'project' ? path.join(cwd, '.pi') : agentDir; }
function localSource(source, base) {
  return !/^(npm:|git:|https?:|ssh:|git@)/.test(source) ? path.resolve(base, source.replace(/^~\//, `${process.env.HOME ?? ''}/`)) : source;
}
function overrideState(entries, patterns, emptyMeansOff = false) {
  if (!entries) return 'inherit';
  if (!entries.length && emptyMeansOff) return 'off';
  let state = 'inherit';
  for (const entry of entries) if (patterns.has(strip(entry))) state = /^[!-]/.test(entry) ? 'off' : 'on';
  return state;
}
function enrichCatalog(items, resources, personal, settings, agentDir, cwd, trusted) {
  const personalMap = new Map();
  for (const kind of Object.keys(PATH_SETTERS)) for (const r of personal[kind] ?? []) personalMap.set(resourceKey(kind, r.path), r);
  // A project filter can mask a personal resource; keep the personal identity so
  // the inspector can edit the default separately from its project override.
  for (const item of items.filter(i => PATH_SETTERS[i.kind])) {
    const inherited = personalMap.get(resourceKey(item.kind, item.path));
    item.personalEnabled = inherited?.enabled ?? null;
    item.personalMetadata = inherited?.metadata;
    item.inherited = !!inherited;
    item.originScope = inherited ? 'user' : 'project';
    const project = settings.getProjectSettings();
    let entries;
    let patterns;
    let emptyOff = false;
    if (item.metadata.origin === 'package') {
      const source = localSource(item.metadata.source, settingsBase(item.scope, agentDir, cwd));
      const pkg = (project.packages ?? []).find(p => localSource(sourceOf(p), settingsBase('project', agentDir, cwd)) === source);
      entries = object(pkg) ? pkg[item.kind] : undefined;
      patterns = new Set([resourcePattern(item, agentDir, cwd)]);
      emptyOff = object(pkg) && pkg.autoload !== false;
    } else {
      entries = project[item.kind];
      patterns = new Set([item.path, path.relative(settingsBase('project', agentDir, cwd), item.path).split(path.sep).join('/'), resourcePattern(item, agentDir, cwd)]);
    }
    item.projectState = trusted ? overrideState(entries, patterns, emptyOff) : 'inherit';
    item.sourceLabel = item.metadata.source === 'builtin' ? 'Built into Pi' : item.metadata.origin === 'package' ? item.metadata.source : 'Local resource';
    item.issue = false;
    item.owner = item.metadata.origin === 'package' ? hash(localSource(item.metadata.source, settingsBase(item.scope, agentDir, cwd))) : '';
  }
  for (const item of items.filter(i => i.kind === 'packages')) {
    item.owner = hash(localSource(item.source, settingsBase(item.scope, agentDir, cwd)));
    item.installed = !!item.path && fs.existsSync(item.path);
    item.issue = !item.installed;
    item.sourceLabel = item.source;
    item.originScope = item.scope;
    item.inherited = false;
    item.projectState = 'inherit';
    item.version = '';
    if (item.installed) {
      try {
        const manifest = readConfig(path.join(item.path, 'package.json'));
        if (typeof manifest.version === 'string') item.version = manifest.version.slice(0, 80);
        if (typeof manifest.name === 'string' && manifest.name) item.name = manifest.name.slice(0, 160);
      } catch {}
    }
    item.resourceCount = items.filter(i => i.kind !== 'packages' && i.owner === item.owner).length;
  }
  const globalMcp = readConfigSafe(path.join(agentDir, 'mcp.json'));
  const projectMcp = trusted ? readConfigSafe(path.join(cwd, '.pi/mcp.json')) : {};
  for (const item of items.filter(i => i.kind === 'mcp')) {
    const base = globalMcp.mcpServers?.[item.name];
    const project = projectMcp.mcpServers?.[item.name];
    const override = object(project) && !project.command && !project.url && !project.type;
    const inherited = object(base) && (item.scope === 'user' || override);
    item.inherited = inherited;
    item.personalEnabled = inherited ? base.enabled !== false : null;
    item.originScope = inherited ? 'user' : item.scope;
    item.projectState = object(project) && typeof project.enabled === 'boolean' ? project.enabled ? 'on' : 'off' : 'inherit';
    const effective = item.scope === 'user' && override ? { ...base, ...project } : item.scope === 'user' ? base : override ? { ...base, ...project } : project;
    item.enabled = object(effective) && effective.enabled !== false;
    item.transport = effective?.url ? 'HTTP' : effective?.command ? 'stdio' : 'Unknown';
    item.exposure = ['direct', 'codemode', 'deferred', 'hidden'].includes(effective?.exposure) ? effective.exposure : 'codemode';
    item.issue = !object(effective) || (!effective.url && !effective.command);
    item.sourceLabel = inherited ? 'Personal MCP configuration' : 'Project MCP configuration';
    item.owner = '';
  }
  // One row per effective server. Keep a project's actual declaration when it
  // replaces the personal one; inherited-only patches remain linked to personal.
  const projectNames = new Set(items.filter(i => i.kind === 'mcp' && i.scope === 'project').map(i => i.name));
  for (let i = items.length - 1; i >= 0; i--) if (items[i].kind === 'mcp' && items[i].scope === 'user' && projectNames.has(items[i].name)) items.splice(i, 1);
}
function readConfigSafe(file) { try { return readConfig(file); } catch { return {}; } }
function assertRevision(request, context) {
  if (request.revision !== context.currentRevision) fail('Configuration changed. Refresh and review the action again.');
}
function selectedResource(request, context) {
  const item = context.items.find(i => i.id === request.id);
  if (!item) fail('Resource no longer exists. Refresh before retrying.');
  return item;
}
function inspectResource(request, context) {
  assertRevision(request, context);
  const item = selectedResource(request, context);
  if (!['skills', 'prompts', 'extensions', 'themes'].includes(item.kind) || item.path.startsWith('builtin:')) fail('This resource has no local preview.');
  let file = item.path;
  if (fs.statSync(file).isDirectory()) file = path.join(file, 'SKILL.md');
  assertWritableConfig(file); // same no-symlink traversal rule for reads
  const stat = fs.statSync(file);
  if (!stat.isFile() || stat.size > 65536) fail('Preview is limited to regular UTF-8 files under 64 KiB.');
  const bytes = fs.readFileSync(file);
  let content;
  try { content = new TextDecoder('utf-8', { fatal: true }).decode(bytes); } catch { fail('Preview requires UTF-8 text.'); }
  if (content.includes('\0')) fail('Binary resources cannot be previewed.');
  return { id: item.id, content, path: file };
}
function setResourceSelection(config, item, scope, state, context) {
  const enabled = state === 'on';
  const meta = scope === 'user' && item.personalMetadata ? item.personalMetadata : item.metadata;
  if (!meta) fail('Resource metadata is unavailable. Refresh before retrying.');
  const topBase = settingsBase(scope, context.agentDir, context.cwd);
  const inherited = scope === 'project' && item.inherited;
  if (meta.origin === 'package') {
    config.packages ??= [];
    const originalSource = localSource(meta.source, settingsBase(meta.scope, context.agentDir, context.cwd));
    let index = config.packages.findIndex(p => localSource(sourceOf(p), topBase) === originalSource);
    if (index < 0) {
      if (state === 'inherit') return;
      if (!inherited) fail('Package configuration changed. Refresh before retrying.');
      const source = originalSource === meta.source ? meta.source : path.relative(topBase, originalSource) || '.';
      config.packages.push({ source, autoload: false });
      index = config.packages.length - 1;
    }
    const pkg = typeof config.packages[index] === 'string' ? { source: config.packages[index] } : config.packages[index];
    const pattern = path.relative(meta.baseDir ?? path.dirname(item.path), item.path).split(path.sep).join('/');
    const entries = (pkg[item.kind] ?? []).filter(p => strip(p) !== pattern);
    if (state !== 'inherit') entries.push(`${enabled ? '+' : '-'}${pattern}`);
    if (entries.length) pkg[item.kind] = entries;
    else delete pkg[item.kind];
    if (!Object.keys(PATH_SETTERS).some(k => pkg[k] !== undefined)) {
      if (pkg.autoload === false) config.packages.splice(index, 1);
      else config.packages[index] = pkg.source;
    } else config.packages[index] = pkg;
  } else {
    const pattern = item.path.startsWith('builtin:') || inherited ? item.path : path.relative(meta.baseDir ?? topBase, item.path).split(path.sep).join('/');
    const targets = new Set([pattern, item.path, path.relative(topBase, item.path).split(path.sep).join('/')]);
    let entries = (config[item.kind] ?? []).filter(p => !( /^[!+-]/.test(p) && targets.has(strip(p))) && !(state === 'inherit' && inherited && p === pattern));
    if (state !== 'inherit') {
      if (inherited && !item.path.startsWith('builtin:') && !entries.includes(pattern)) entries.push(pattern);
      entries.push(`${enabled ? '+' : '-'}${pattern}`);
    }
    config[item.kind] = entries;
  }
}
function redactConfig(config, mcp) {
  if (!mcp) return config;
  const result = structuredClone(config);
  for (const server of Object.values(result.mcpServers ?? {})) if (object(server)) {
    for (const key of Object.keys(server)) if (!['enabled', 'exposure', 'toolExposure', 'type', 'timeout'].includes(key)) server[key] = '[redacted: open config to inspect]';
  }
  return result;
}
function changedFields(before, after, mcp) {
  const keys = mcp ? ['mcpServers'] : [...Object.keys(PATH_SETTERS), 'packages'];
  const a = {}, b = {};
  for (const key of keys) if (JSON.stringify(before[key]) !== JSON.stringify(after[key])) {
    if (mcp) {
      a[key] = {}; b[key] = {};
      const names = new Set([...Object.keys(before[key] ?? {}), ...Object.keys(after[key] ?? {})]);
      for (const name of names) if (JSON.stringify(before[key]?.[name]) !== JSON.stringify(after[key]?.[name])) {
        if (Object.hasOwn(before[key] ?? {}, name)) a[key][name] = before[key][name];
        if (Object.hasOwn(after[key] ?? {}, name)) b[key][name] = after[key][name];
      }
    } else { if (before[key] !== undefined) a[key] = before[key]; if (after[key] !== undefined) b[key] = after[key]; }
  }
  return { before: JSON.stringify(redactConfig(a, mcp), null, 2), after: JSON.stringify(redactConfig(b, mcp), null, 2), redacted: mcp };
}
function planOperation(op, context, configs) {
  const { agentDir, cwd, trusted, items, validateMcp } = context;
  if (!['user', 'project'].includes(op.scope)) fail('Choose a valid configuration scope.');
  if (op.scope === 'project' && !trusted) fail('Project resources are not trusted. Use Pi /trust first.');
  const item = op.id ? selectedResource(op, context) : undefined;
  const mcp = item?.kind === 'mcp' || op.action.startsWith('mcp-');
  const file = path.join(settingsBase(op.scope, agentDir, cwd), mcp ? 'mcp.json' : 'settings.json');
  assertWritableConfig(file);
  const before = structuredClone(configs.get(file) ?? readConfig(file));
  const after = structuredClone(before);
  let operation = '';
  if (op.action === 'select') {
    if (!item || item.kind === 'packages') fail('This resource does not support selection.');
    if (!['on', 'off', 'inherit'].includes(op.state)) fail('Choose enable, disable or inherit.');
    if (op.scope === 'user' && !item.inherited) fail('This resource has no personal default.');
    if (op.scope === 'user' && op.state === 'inherit') fail('Personal defaults must be enabled or disabled.');
    if (mcp) {
      after.mcpServers ??= {};
      const server = after.mcpServers[item.name];
      if (op.scope === 'project' && item.inherited) {
        if (op.state === 'inherit') {
          if (object(server) && (server.command || server.url || server.type)) fail('A project server definition cannot be reset as an inherited override.');
          if (object(server)) { delete server.enabled; if (!Object.keys(server).length) delete after.mcpServers[item.name]; }
        } else { after.mcpServers[item.name] = { ...server, enabled: op.state === 'on' }; }
      } else {
        if (!object(server)) fail('MCP server definition changed. Refresh before retrying.');
        server.enabled = op.state === 'on';
      }
    } else setResourceSelection(after, item, op.scope, op.state, context);
  } else if (op.action === 'register') {
    if (!PATH_SETTERS[op.kind]) fail('Choose a local resource type.');
    let target = op.source?.trim();
    if (!target || target.length > 4096) fail('Enter a local resource path.');
    target = path.resolve(cwd, target.replace(/^~\//, `${process.env.HOME ?? ''}/`));
    if (!fs.existsSync(target)) fail('Local resource path does not exist.');
    after[op.kind] = [...(after[op.kind] ?? []).filter(p => strip(p) !== target), target, `+${target}`];
  } else if (op.action === 'mcp-add' || op.action === 'mcp-edit') {
    const name = item?.name ?? op.name?.trim();
    if (!name || !/^[a-zA-Z0-9_-]+$/.test(name) || ['__proto__', 'constructor', 'prototype'].includes(name)) fail('Choose a valid, non-reserved MCP server name.');
    let patch;
    try { patch = JSON.parse(op.definition); } catch { fail('Server definition must be valid JSON.'); }
    if (!object(patch)) fail('Server definition must be a JSON object.');
    after.mcpServers ??= {};
    if (!object(after.mcpServers)) fail('Invalid mcpServers object.');
    if (op.action === 'mcp-add' && Object.hasOwn(after.mcpServers, name)) fail('Server already exists. Select it to edit.');
    const next = { ...(op.action === 'mcp-edit' ? after.mcpServers[name] : {}), ...patch };
    const override = op.scope === 'project' && !next.command && !next.url && !next.type;
    const global = configs.get(path.join(agentDir, 'mcp.json')) ?? readConfigSafe(path.join(agentDir, 'mcp.json'));
    const base = override ? global.mcpServers?.[name] : undefined;
    if (override && (!object(base) || Object.keys(next).some(k => !['enabled', 'exposure', 'toolExposure'].includes(k)))) fail('Project overrides can only set enabled, exposure and toolExposure.');
    if (typeof validateMcp(name, { ...base, ...next }) === 'string') fail('Invalid MCP definition. Check transport, command/args or URL and exposure.');
    if (op.scope === 'project' && next.auth) fail('Provider auth is only allowed in personal MCP configuration.');
    if (Object.keys(after.mcpServers).some(other => other !== name && other.replaceAll('-', '_') === name.replaceAll('-', '_'))) fail('Another MCP server uses the same normalized name.');
    after.mcpServers[name] = next;
  } else if (op.action === 'install') {
    const source = op.source?.trim();
    if (!source || source.length > 4096 || source.startsWith('-')) fail('Enter a valid package source.');
    operation = `Install ${source}. Dependencies may execute code. Contents are unknown until installed. Pi normalizes the declaration when installation succeeds.`;
  } else if (op.action === 'remove') {
    if (!item?.removable) fail('Built-in resources cannot be removed. Disable them instead.');
    if (item.kind === 'packages') {
      if (op.scope !== item.scope) fail('Package removal must target its defining scope.');
      operation = `Uninstall ${item.source}. Managed installation and declaration are removed; local source files are kept.`;
    } else if (mcp) {
      if (!Object.hasOwn(after.mcpServers ?? {}, item.name)) fail('MCP entry no longer exists in this scope.');
      delete after.mcpServers[item.name];
    } else setResourceSelection(after, item, op.scope, 'off', context);
  } else fail('Unknown resource operation.');
  configs.set(file, after);
  return { op, item, file, before, after, operation, title: item?.name ?? op.name ?? op.source ?? op.kind };
}
async function batchRequest(request, context) {
  assertRevision(request, context);
  if (!Array.isArray(request.operations) || !request.operations.length || request.operations.length > 32 || request.operations.some(op => !object(op) || typeof op.action !== 'string')) fail('Review between 1 and 32 valid resource changes.');
  const configs = new Map();
  const steps = request.operations.map(op => planOperation(op, context, configs));
  for (let i = 0; i < steps.length; i++) {
    if (steps[i].op.action === 'remove' && steps[i].item && steps.slice(i + 1).some(s => s.op.id === steps[i].op.id)) fail('A removed resource cannot be edited later in the same review.');
  }
  // A package manager normalizes its own declaration. Mixing that with a later
  // planned package-array replacement could discard its output. Keep such edits
  // in a separate reviewed batch rather than pretend they are transactional.
  for (let i = 0; i < steps.length; i++) if (steps[i].operation) {
    if (steps.slice(i + 1).some(s => s.file === steps[i].file && JSON.stringify(s.before.packages) !== JSON.stringify(s.after.packages))) fail('Apply package installs/removals separately from later package-filter changes in the same scope.');
  }
  const token = hash(JSON.stringify({ revision: context.currentRevision, operations: request.operations, plans: steps.map(s => [s.file, s.after, s.operation]) }));
  const summaries = steps.map((s, index) => {
    const fields = changedFields(s.before, s.after, s.file.endsWith('mcp.json'));
    const diff = context.sdk.generateUnifiedPatch ? context.sdk.generateUnifiedPatch(s.file, fields.before, fields.after, 3) : '';
    return { index, title: s.title, file: s.file, operation: s.operation, ...fields, diff,
      affected: s.item?.kind === 'packages' ? context.items.filter(i => i.kind !== 'packages' && i.owner === s.item.owner).map(i => i.name) : [] };
  });
  if (JSON.stringify(summaries).length > 2 * 1024 * 1024) fail('Review is too large. Split the changes into smaller batches.');
  if (request.action === 'preview') return { token, steps: summaries };
  if (request.token !== token) fail('Review is out of date. Review the changes again before applying.');
  context.settings.applyOverrides({ npmCommand: context.npmCommand ?? ['npm'] });
  const results = [];
  let expectedRevision = context.currentRevision;
  for (const [index, step] of steps.entries()) {
    try {
      if (revision(context.agentDir, context.cwd, context.trusted) !== expectedRevision) fail('Configuration changed while applying. Remaining operations were not run.');
      if (step.operation) {
        if (context.settings.reload) await context.settings.reload();
        if (revision(context.agentDir, context.cwd, context.trusted) !== expectedRevision) fail('Configuration changed before the package operation. Refresh before retrying.');
        if (step.op.action === 'install') await context.manager.installAndPersist(step.op.source.trim(), { local: step.op.scope === 'project' });
        else await context.manager.removeAndPersist(step.item.source, { local: step.op.scope === 'project' });
        await context.settings.flush();
        if (context.settings.drainErrors().length) fail('Pi could not save package configuration.');
      } else editConfig(step.file, current => {
        // Patch only the reviewed resource fields; preserve unrelated config.
        const keys = step.file.endsWith('mcp.json') ? ['mcpServers'] : [...Object.keys(PATH_SETTERS), 'packages'];
        for (const key of keys) if (JSON.stringify(step.before[key]) !== JSON.stringify(step.after[key])) {
          if (JSON.stringify(current[key]) !== JSON.stringify(step.before[key])) fail('Configuration changed while applying. Refresh before retrying.');
          if (step.after[key] === undefined) delete current[key]; else current[key] = step.after[key];
        }
      });
      results.push({ index, success: true, message: 'Applied' });
      expectedRevision = revision(context.agentDir, context.cwd, context.trusted);
    } catch (error) {
      results.push({ index, success: false, message: error instanceof ResourceError ? error.message : 'Operation failed or partially completed. Refresh and review before retrying; no automatic retry.' });
      break;
    }
  }
  return { results, complete: results.length === steps.length && results.every(r => r.success) };
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
