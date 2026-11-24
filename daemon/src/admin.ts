import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { piAgentDir, type DaemonConfig } from './config.ts';

export interface CloudAdminStatus {
  has_pi_auth: boolean;
  pi_auth_providers: string[];
  has_git_ssh_key: boolean;
  git_key_mode: 'dedicated' | 'copied' | null;
  git_public_key: string | null;
  git_user_name: string | null;
  git_user_email: string | null;
  skills: string[];
  mcp_servers: string[];
  service_creds: string[];
}

export interface IncomingServiceCred {
  id: string;
  content: string;
}

const SERVICE_CRED_RELATIVE_PATHS: Record<string, string> = {
  gh: path.join('.config', 'gh', 'hosts.yml'),
  railway: path.join('.railway', 'config.json'),
  render: path.join('.render', 'cli.yaml'),
  vercel: path.join('.config', 'vercel', 'auth.json'),
  fly: path.join('.fly', 'config.yml'),
  wrangler: path.join('.config', '.wrangler', 'config', 'default.toml'),
  npm: '.npmrc',
};

function serviceCredHome(): string {
  return process.env.PUI_DAEMON_HOME_DIR || os.homedir();
}

function detectCloudServiceCreds(): string[] {
  const home = serviceCredHome();
  const found: string[] = [];
  for (const [id, rel] of Object.entries(SERVICE_CRED_RELATIVE_PATHS)) {
    const full = path.join(home, rel);
    if (fs.existsSync(full)) {
      found.push(id);
    }
  }
  return found.sort();
}

export function writeCloudServiceCreds(
  items: IncomingServiceCred[],
  removeIds: string[] = [],
): { service_creds: string[] } {
  const home = serviceCredHome();
  for (const id of removeIds.slice(0, 16)) {
    const rel = SERVICE_CRED_RELATIVE_PATHS[id];
    if (!rel) continue;
    const dest = path.join(home, rel);
    if (fs.existsSync(dest)) {
      try {
        fs.unlinkSync(dest);
      } catch {
        // ignore
      }
    }
  }
  for (const item of items.slice(0, 16)) {
    const rel = SERVICE_CRED_RELATIVE_PATHS[item.id];
    if (!rel) continue;
    if (typeof item.content !== 'string' || !item.content.trim()) continue;
    if (Buffer.byteLength(item.content, 'utf8') > 128 * 1024) continue;
    const dest = path.join(home, rel);
    fs.mkdirSync(path.dirname(dest), { recursive: true, mode: 0o700 });
    fs.writeFileSync(dest, item.content, { mode: 0o600 });
    try {
      fs.chmodSync(dest, 0o600);
    } catch {
      // ignore
    }
  }
  return { service_creds: detectCloudServiceCreds() };
}

function skillsDir(): string {
  return process.env.PUI_DAEMON_SKILLS_DIR || path.join(os.homedir(), '.agents', 'skills');
}

function sshDir(): string {
  return process.env.PUI_DAEMON_SSH_DIR || path.join(os.homedir(), '.ssh');
}

function cloudKeyPath(): string {
  return path.join(sshDir(), 'id_ed25519_pui_cloud');
}

function gitConfigGet(key: string): string | null {
  try {
    const out = execFileSync('git', ['config', '--global', '--get', key], {
      encoding: 'utf8',
      timeout: 3000,
    }).trim();
    return out || null;
  } catch {
    return null;
  }
}

export function getAdminStatus(): CloudAdminStatus {
  const authPath = path.join(piAgentDir(), 'auth.json');
  let has_pi_auth = false;
  let pi_auth_providers: string[] = [];
  if (fs.existsSync(authPath)) {
    try {
      const parsed = JSON.parse(fs.readFileSync(authPath, 'utf8')) as unknown;
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        pi_auth_providers = Object.keys(parsed as Record<string, unknown>).sort();
        has_pi_auth = pi_auth_providers.length > 0;
      }
    } catch {
      // ignore malformed file
    }
  }

  const pubPath = `${cloudKeyPath()}.pub`;
  let git_public_key: string | null = null;
  if (fs.existsSync(pubPath)) {
    try {
      git_public_key = fs.readFileSync(pubPath, 'utf8').trim() || null;
    } catch {
      git_public_key = null;
    }
  }

  const skills: string[] = [];
  const sDir = skillsDir();
  if (fs.existsSync(sDir)) {
    try {
      for (const ent of fs.readdirSync(sDir, { withFileTypes: true })) {
        if (ent.isDirectory() && !ent.name.startsWith('.')) {
          skills.push(ent.name);
        }
      }
      skills.sort();
    } catch {
      // ignore
    }
  }

  const mcp_servers: string[] = [];
  const mcpPath = path.join(piAgentDir(), 'mcp.json');
  if (fs.existsSync(mcpPath)) {
    try {
      const parsed = JSON.parse(fs.readFileSync(mcpPath, 'utf8')) as Record<string, unknown>;
      const servers =
        parsed.mcpServers && typeof parsed.mcpServers === 'object'
          ? (parsed.mcpServers as Record<string, unknown>)
          : parsed;
      for (const key of Object.keys(servers)) {
        if (key !== 'mcpServers') mcp_servers.push(key);
      }
      mcp_servers.sort();
    } catch {
      // ignore
    }
  }

  const hasKey = Boolean(git_public_key && fs.existsSync(cloudKeyPath()));
  const git_key_mode: 'dedicated' | 'copied' | null = !hasKey
    ? null
    : git_public_key?.trim().endsWith('pui-cloud-git')
      ? 'dedicated'
      : 'copied';

  return {
    has_pi_auth,
    pi_auth_providers,
    has_git_ssh_key: hasKey,
    git_key_mode,
    git_public_key,
    git_user_name: gitConfigGet('user.name'),
    git_user_email: gitConfigGet('user.email'),
    skills,
    mcp_servers,
    service_creds: detectCloudServiceCreds(),
  };
}

/**
 * Stores Pi provider credentials (`auth.json`) pushed from Pui Desktop over Tailscale.
 * Merges into existing auth.json if `merge` is true, always preserving 0600 file permissions.
 */
export function writePiAuth(
  authPayload: Record<string, unknown>,
  merge = true,
): { providers: string[] } {
  const dir = piAgentDir();
  fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
  const authPath = path.join(dir, 'auth.json');

  let existing: Record<string, unknown> = {};
  if (merge && fs.existsSync(authPath)) {
    try {
      const parsed = JSON.parse(fs.readFileSync(authPath, 'utf8')) as unknown;
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        existing = parsed as Record<string, unknown>;
      }
    } catch {
      existing = {};
    }
  }

  const next = { ...existing, ...authPayload };
  fs.writeFileSync(authPath, JSON.stringify(next, null, 2) + '\n', { mode: 0o600 });
  try {
    fs.chmodSync(authPath, 0o600);
  } catch {
    // ignore on platforms without chmod
  }
  return { providers: Object.keys(next).sort() };
}

/**
 * Ensures a dedicated revocable ed25519 SSH keypair exists on this cloud host for GitHub/GitLab,
 * configures `~/.ssh/config` + `known_hosts` for github.com, and optionally sets global git author.
 * Returns ONLY the public key (never the private key).
 */
export function setupCloudGit(opts: {
  userName?: string;
  userEmail?: string;
  regenerateKey?: boolean;
  privateKey?: string;
  publicKey?: string;
}): { public_key: string; user_name: string | null; user_email: string | null; mode: 'copied' | 'dedicated' } {
  const dir = sshDir();
  fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
  const keyFile = cloudKeyPath();
  const pubFile = `${keyFile}.pub`;

  let mode: 'copied' | 'dedicated' = 'dedicated';
  if (opts.privateKey && opts.privateKey.trim()) {
    const priv = opts.privateKey.trim();
    if (!priv.startsWith('-----BEGIN ') || !priv.includes('PRIVATE KEY-----')) {
      throw Object.assign(new Error('Invalid SSH private key format.'), { status: 400 });
    }
    fs.writeFileSync(keyFile, priv + '\n', { mode: 0o600 });
    fs.chmodSync(keyFile, 0o600);
    if (opts.publicKey && opts.publicKey.trim()) {
      fs.writeFileSync(pubFile, opts.publicKey.trim() + '\n', { mode: 0o644 });
    } else {
      const derived = execFileSync('ssh-keygen', ['-y', '-P', '', '-f', keyFile], {
        encoding: 'utf8',
        timeout: 5000,
      }).trim();
      fs.writeFileSync(pubFile, derived + '\n', { mode: 0o644 });
    }
    fs.chmodSync(pubFile, 0o644);
    mode = 'copied';
  } else if (opts.regenerateKey || !fs.existsSync(keyFile) || !fs.existsSync(pubFile)) {
    if (fs.existsSync(keyFile)) fs.unlinkSync(keyFile);
    if (fs.existsSync(pubFile)) fs.unlinkSync(pubFile);
    execFileSync(
      'ssh-keygen',
      ['-t', 'ed25519', '-f', keyFile, '-N', '', '-C', 'pui-cloud-git'],
      { timeout: 10_000 },
    );
    fs.chmodSync(keyFile, 0o600);
    fs.chmodSync(pubFile, 0o644);
  }

  // Ensure SSH uses this dedicated key for github.com & gitlab.com without strict interactive prompt.
  const cfgPath = path.join(dir, 'config');
  const existingCfg = fs.existsSync(cfgPath) ? fs.readFileSync(cfgPath, 'utf8') : '';
  const marker = '# pui-cloud-git-managed';
  if (!existingCfg.includes(marker)) {
    const block = [
      '',
      marker,
      'Host github.com gitlab.com',
      `  IdentityFile ${keyFile}`,
      '  IdentitiesOnly yes',
      '  StrictHostKeyChecking accept-new',
      '',
    ].join('\n');
    fs.writeFileSync(cfgPath, existingCfg + block, { mode: 0o600 });
  }

  if (opts.userName && opts.userName.trim()) {
    execFileSync('git', ['config', '--global', 'user.name', opts.userName.trim()], {
      timeout: 3000,
    });
  }
  if (opts.userEmail && opts.userEmail.trim()) {
    execFileSync('git', ['config', '--global', 'user.email', opts.userEmail.trim()], {
      timeout: 3000,
    });
  }

  return {
    public_key: fs.readFileSync(pubFile, 'utf8').trim(),
    user_name: gitConfigGet('user.name'),
    user_email: gitConfigGet('user.email'),
    mode,
  };
}

export function writeCloudMcps(
  mcpServers: Record<string, unknown>,
  merge = true,
): { mcp_servers: string[] } {
  const dir = piAgentDir();
  fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
  const mcpPath = path.join(dir, 'mcp.json');

  let existing: Record<string, unknown> = {};
  if (merge && fs.existsSync(mcpPath)) {
    try {
      const parsed = JSON.parse(fs.readFileSync(mcpPath, 'utf8')) as Record<string, unknown>;
      if (parsed.mcpServers && typeof parsed.mcpServers === 'object') {
        existing = parsed.mcpServers as Record<string, unknown>;
      }
    } catch {
      existing = {};
    }
  }

  const merged = { ...existing, ...mcpServers };
  fs.writeFileSync(mcpPath, JSON.stringify({ mcpServers: merged }, null, 2) + '\n', {
    mode: 0o600,
  });
  try {
    fs.chmodSync(mcpPath, 0o600);
  } catch {
    // ignore
  }
  return { mcp_servers: Object.keys(merged).sort() };
}

export interface IncomingSkillBundle {
  name: string;
  files: { path: string; content: string }[];
}

export function removeCloudGitKey(): { removed: boolean } {
  const keyFile = cloudKeyPath();
  const pubFile = `${keyFile}.pub`;
  let removed = false;
  for (const f of [keyFile, pubFile]) {
    if (fs.existsSync(f)) {
      fs.unlinkSync(f);
      removed = true;
    }
  }
  return { removed };
}

export function writeCloudSkills(
  bundles: IncomingSkillBundle[],
  removeNames: string[] = [],
): { skills: string[] } {
  const root = skillsDir();
  fs.mkdirSync(root, { recursive: true });
  const resolvedBase = fs.realpathSync(root);

  for (const rawName of removeNames.slice(0, 60)) {
    const safeName = rawName.trim().replace(/[^a-zA-Z0-9._-]/g, '-');
    if (!safeName || safeName === '.' || safeName === '..' || safeName.startsWith('.')) {
      continue;
    }
    const target = path.join(resolvedBase, safeName);
    if (fs.existsSync(target)) {
      const resolvedTarget = fs.realpathSync(target);
      if (resolvedTarget.startsWith(resolvedBase + path.sep)) {
        fs.rmSync(resolvedTarget, { recursive: true, force: true });
      }
    }
  }

  for (const bundle of bundles.slice(0, 60)) {
    const safeName = bundle.name.trim().replace(/[^a-zA-Z0-9._-]/g, '-');
    if (!safeName || safeName === '.' || safeName === '..' || safeName.startsWith('.')) {
      continue;
    }
    const skillRoot = path.join(root, safeName);
    fs.mkdirSync(skillRoot, { recursive: true });
    const resolvedRoot = fs.realpathSync(skillRoot);

    for (const file of (bundle.files ?? []).slice(0, 40)) {
      const rel = file.path.replace(/\\/g, '/').replace(/^\/+/, '');
      if (!rel || rel.split('/').some((seg) => !seg || seg === '.' || seg === '..')) {
        continue;
      }
      if (typeof file.content !== 'string' || file.content.length > 256 * 1024) {
        continue;
      }
      const target = path.join(resolvedRoot, rel);
      const targetDir = path.dirname(target);
      fs.mkdirSync(targetDir, { recursive: true });
      const resolvedDir = fs.realpathSync(targetDir);
      if (resolvedDir !== resolvedRoot && !resolvedDir.startsWith(resolvedRoot + path.sep)) {
        continue;
      }
      fs.writeFileSync(target, file.content, 'utf8');
    }
  }

  const present: string[] = [];
  for (const ent of fs.readdirSync(root, { withFileTypes: true })) {
    if (ent.isDirectory() && !ent.name.startsWith('.')) {
      present.push(ent.name);
    }
  }
  return { skills: present.sort() };
}

export function normalizeGitRemote(input: string): string | null {
  const trimmed = input.trim();
  if (!trimmed || trimmed.includes('..') || trimmed.startsWith('-')) return null;
  if (/^[a-zA-Z0-9._-]+\/[a-zA-Z0-9._-]+$/.test(trimmed)) {
    return `git@github.com:${trimmed}.git`;
  }
  const ok =
    /^https:\/\/[a-zA-Z0-9._-]+(\/[a-zA-Z0-9._~:/?#[\]@!$&'()*+,;=%-]+)$/.test(trimmed) ||
    /^git@[a-zA-Z0-9._-]+:[a-zA-Z0-9._/-]+(?:\.git)?$/.test(trimmed) ||
    /^ssh:\/\/git@[a-zA-Z0-9._-]+(?::\d+)?\/[a-zA-Z0-9._/-]+(?:\.git)?$/.test(trimmed);
  return ok ? trimmed : null;
}

export function validateGitCloneUrl(url: string): string {
  const normalized = normalizeGitRemote(url);
  if (!normalized) {
    throw Object.assign(new Error('Unsupported or unsafe Git clone URL.'), { status: 400 });
  }
  return normalized;
}

export function inferRepoDirName(url: string, explicitName?: string): string {
  const candidate =
    explicitName?.trim() ||
    url
      .replace(/\.git$/i, '')
      .split(/[/:]/)
      .filter(Boolean)
      .pop() ||
    'repo';
  const safe = candidate.replace(/[^a-zA-Z0-9._-]/g, '-').replace(/^\.+/, '');
  if (!safe || safe === '.' || safe === '..') {
    throw Object.assign(new Error('Invalid repository folder name.'), { status: 400 });
  }
  return safe;
}

export function cloneRepoIntoWorkspace(
  config: DaemonConfig,
  url: string,
  opts: { name?: string; branch?: string } = {},
): { path: string; already_exists: boolean } {
  const safeUrl = validateGitCloneUrl(url);
  const dirName = inferRepoDirName(safeUrl, opts.name);
  fs.mkdirSync(config.workspaces_dir, { recursive: true });
  const target = path.join(config.workspaces_dir, dirName);

  if (fs.existsSync(target)) {
    return { path: fs.realpathSync(target), already_exists: true };
  }

  const args = ['clone'];
  if (opts.branch && /^[a-zA-Z0-9._/-]+$/.test(opts.branch) && !opts.branch.startsWith('-')) {
    args.push('--branch', opts.branch);
  }
  args.push('--', safeUrl, target);

  execFileSync('git', args, {
    cwd: config.workspaces_dir,
    timeout: 120_000,
    stdio: 'pipe',
  });

  return { path: fs.realpathSync(target), already_exists: false };
}
