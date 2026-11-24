import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

export interface DaemonConfig {
  host_name: string;
  port: number;
  bind_mode: 'tailscale' | 'localhost' | 'all';
  token: string;
  use_tls_in_qr: boolean;
  workspaces_dir: string;
  extra_projects: string[];
}

export function daemonHomeDir(): string {
  return process.env.PUI_DAEMON_HOME || path.join(os.homedir(), '.pui-daemon');
}

export function piAgentDir(): string {
  return process.env.PI_CODING_AGENT_DIR || path.join(os.homedir(), '.pi', 'agent');
}

export function isTailscaleIpv4(ip: string): boolean {
  const parts = ip.split('.').map(Number);
  if (parts.length !== 4 || parts.some((n) => !Number.isInteger(n) || n < 0 || n > 255)) {
    return false;
  }
  // CGNAT 100.64.0.0/10
  return parts[0] === 100 && parts[1] >= 64 && parts[1] <= 127;
}

export function detectTailscaleIp(): string | null {
  const ifaces = os.networkInterfaces();
  for (const entries of Object.values(ifaces)) {
    if (!entries) continue;
    for (const entry of entries) {
      if (entry.family === 'IPv4' && !entry.internal && isTailscaleIpv4(entry.address)) {
        return entry.address;
      }
    }
  }
  return null;
}

export function loadOrCreateConfig(): DaemonConfig {
  const home = daemonHomeDir();
  fs.mkdirSync(home, { recursive: true });
  const cfgPath = path.join(home, 'config.json');

  let stored: Partial<DaemonConfig> = {};
  if (fs.existsSync(cfgPath)) {
    try {
      stored = JSON.parse(fs.readFileSync(cfgPath, 'utf8')) as Partial<DaemonConfig>;
    } catch {
      stored = {};
    }
  }

  const envBind = process.env.PUI_DAEMON_BIND?.trim().toLowerCase();
  const bind_mode: DaemonConfig['bind_mode'] =
    envBind === 'all' || envBind === '0.0.0.0'
      ? 'all'
      : envBind === 'localhost' || envBind === '127.0.0.1'
        ? 'localhost'
        : envBind === 'tailscale'
          ? 'tailscale'
          : stored.bind_mode ?? 'tailscale';

  const config: DaemonConfig = {
    host_name: process.env.PUI_DAEMON_NAME?.trim() || stored.host_name || os.hostname() || 'pui-cloud',
    port: Number(process.env.PORT || process.env.PUI_DAEMON_PORT || stored.port || 4817),
    bind_mode,
    token: process.env.PUI_DAEMON_TOKEN?.trim() || stored.token || crypto.randomBytes(24).toString('hex'),
    use_tls_in_qr:
      process.env.PUI_DAEMON_TLS === '1' ||
      process.env.PUI_DAEMON_TLS === 'true' ||
      Boolean(stored.use_tls_in_qr),
    workspaces_dir:
      process.env.PUI_DAEMON_WORKSPACES?.trim() ||
      stored.workspaces_dir ||
      path.join(os.homedir(), 'workspaces'),
    extra_projects: Array.isArray(stored.extra_projects) ? stored.extra_projects : [],
  };

  fs.mkdirSync(config.workspaces_dir, { recursive: true });
  fs.writeFileSync(cfgPath, JSON.stringify(config, null, 2) + '\n', { mode: 0o600 });
  return config;
}

export function resolveBindHost(config: DaemonConfig): { bindIp: string; tailscaleIp: string | null } {
  const ts = detectTailscaleIp();
  if (config.bind_mode === 'localhost') {
    return { bindIp: '127.0.0.1', tailscaleIp: ts };
  }
  if (config.bind_mode === 'all') {
    return { bindIp: '0.0.0.0', tailscaleIp: ts };
  }
  if (ts) {
    return { bindIp: ts, tailscaleIp: ts };
  }
  return { bindIp: '127.0.0.1', tailscaleIp: null };
}

export function buildPairingUri(
  host: string,
  port: number,
  token: string,
  name: string,
  tls: boolean,
): string {
  const params = new URLSearchParams({
    v: '1',
    host,
    port: String(port),
    token,
    name,
    tls: tls ? '1' : '0',
  });
  return `pui://pair?${params.toString()}`;
}

export function discoverProjects(config: DaemonConfig): string[] {
  const found = new Set<string>();
  const addDir = (p: string) => {
    try {
      const real = fs.realpathSync(p);
      if (fs.statSync(real).isDirectory()) {
        found.add(real);
      }
    } catch {
      // ignore missing paths
    }
  };

  for (const extra of config.extra_projects) {
    addDir(extra);
  }

  try {
    const entries = fs.readdirSync(config.workspaces_dir, { withFileTypes: true });
    for (const ent of entries) {
      if (ent.name.startsWith('.')) continue;
      if (ent.isDirectory()) {
        addDir(path.join(config.workspaces_dir, ent.name));
      }
    }
  } catch {
    // ignore
  }

  if (found.size === 0) {
    addDir(config.workspaces_dir);
  }
  return Array.from(found).sort();
}
