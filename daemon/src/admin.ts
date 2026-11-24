import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { piAgentDir, type DaemonConfig } from './config.ts';

export interface CloudAdminStatus {
  has_pi_auth: boolean;
  pi_auth_providers: string[];
  has_git_ssh_key: boolean;
  git_public_key: string | null;
  git_user_name: string | null;
  git_user_email: string | null;
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

  return {
    has_pi_auth,
    pi_auth_providers,
    has_git_ssh_key: Boolean(git_public_key && fs.existsSync(cloudKeyPath())),
    git_public_key,
    git_user_name: gitConfigGet('user.name'),
    git_user_email: gitConfigGet('user.email'),
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
}): { public_key: string; user_name: string | null; user_email: string | null } {
  const dir = sshDir();
  fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
  const keyFile = cloudKeyPath();
  const pubFile = `${keyFile}.pub`;

  if (opts.regenerateKey || !fs.existsSync(keyFile) || !fs.existsSync(pubFile)) {
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
  };
}

export function validateGitCloneUrl(url: string): string {
  const trimmed = url.trim();
  const ok =
    /^https:\/\/[a-zA-Z0-9._-]+(\/[a-zA-Z0-9._~:/?#[\]@!$&'()*+,;=%-]+)$/.test(trimmed) ||
    /^git@[a-zA-Z0-9._-]+:[a-zA-Z0-9._/-]+(?:\.git)?$/.test(trimmed) ||
    /^ssh:\/\/git@[a-zA-Z0-9._-]+(?::\d+)?\/[a-zA-Z0-9._/-]+(?:\.git)?$/.test(trimmed);
  if (!ok || trimmed.includes('..') || trimmed.startsWith('-')) {
    throw Object.assign(new Error('Unsupported or unsafe Git clone URL.'), { status: 400 });
  }
  return trimmed;
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
