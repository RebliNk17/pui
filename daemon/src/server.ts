import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { LiveAgent, type NotificationEvent, type SessionDetail } from './agent.ts';
import {
  cloneRepoIntoWorkspace,
  createCloudWorktree,
  getAdminStatus,
  invalidateGithubSshCheck,
  listCatalogProjects,
  listCloudRepos,
  normalizeGitRemote,
  removeCloudGitKey,
  setupCloudGit,
  writeCatalogProjects,
  writeCloudMcps,
  writeCloudServiceCreds,
  writeCloudSkills,
  writePiAuth,
  type IncomingCatalogProject,
  type IncomingServiceCred,
  type IncomingSkillBundle,
} from './admin.ts';
import { discoverProjects, type DaemonConfig } from './config.ts';
import {
  gitChanges,
  gitDiff,
  listFileTree,
  mimeForPath,
  readFileContent,
  resolveSafeFile,
  searchWorkspaceFiles,
} from './files.ts';
import {
  createEmptySessionFile,
  defaultCloudOrigin,
  extractArtifacts,
  forkSessionFile,
  loadAllMeta,
  parseSessionFile,
  saveAllMeta,
  scanSavedSessions,
  type ChatMeta,
  type SessionOrigin,
  type SessionSummary,
} from './sessions.ts';
import { acceptWebSocket, type WsClient } from './ws.ts';

const PERMISSION_MODES = [
  { key: 'auto', label: 'Auto' },
  { key: 'manual', label: 'Manual' },
  { key: 'acceptEdits', label: 'Accept edits' },
  { key: 'plan', label: 'Plan' },
  { key: 'dontAsk', label: "Don't ask" },
  { key: 'bypassPermissions', label: 'Bypass' },
];

const LOCAL_SLASH_COMMANDS = [
  { name: 'compact', description: 'Compact conversation context', source: 'pui' },
  { name: 'files', description: 'Browse workspace files', source: 'pui' },
  { name: 'model', description: 'Switch model or effort level', source: 'pui' },
];

function projectNameForCwd(cwd: string, meta?: ChatMeta): string {
  if (meta?.repo_dir) {
    const base = path.basename(meta.repo_dir);
    if (base) return base;
  }
  const m = cwd.match(/\/\.worktrees\/([^/]+)\//);
  if (m && m[1]) return m[1];
  return path.basename(cwd) || 'workspace';
}

function constantTimeEq(a: string, b: string): boolean {
  const ba = Buffer.from(a);
  const bb = Buffer.from(b);
  if (ba.length !== bb.length) return false;
  return crypto.timingSafeEqual(ba, bb);
}

function readBody(req: http.IncomingMessage, maxBytes = 16 * 1024 * 1024): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    let total = 0;
    req.on('data', (chunk: Buffer) => {
      total += chunk.length;
      if (total > maxBytes) {
        reject(Object.assign(new Error('Request body too large.'), { status: 413 }));
        req.destroy();
        return;
      }
      chunks.push(chunk);
    });
    req.on('end', () => resolve(Buffer.concat(chunks)));
    req.on('error', reject);
  });
}

function sendJson(res: http.ServerResponse, status: number, payload: unknown): void {
  const body = JSON.stringify(payload);
  res.writeHead(status, {
    'Content-Type': 'application/json; charset=utf-8',
    'Content-Length': String(Buffer.byteLength(body, 'utf8')),
    'Cache-Control': 'no-store',
    'Access-Control-Allow-Origin': '*',
    'Access-Control-Allow-Headers': 'Authorization, Content-Type',
    'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
  });
  res.end(body);
}

export class PuiDaemonServer {
  readonly config: DaemonConfig;
  readonly tailscaleIp: string | null;
  private meta: Record<string, ChatMeta>;
  private liveAgents = new Map<string, LiveAgent>();
  private clients = new Set<WsClient>();
  private revision = 1;
  private server: http.Server;

  constructor(config: DaemonConfig, tailscaleIp: string | null) {
    this.config = config;
    this.tailscaleIp = tailscaleIp;
    this.meta = loadAllMeta();
    this.server = http.createServer((req, res) => {
      void this.handleHttp(req, res);
    });
    this.server.on('upgrade', (req, socket, head) => {
      this.handleUpgrade(req, socket, head);
    });
  }

  listen(bindIp: string, port = this.config.port): Promise<{ host: string; port: number }> {
    return new Promise((resolve, reject) => {
      this.server.once('error', reject);
      this.server.listen(port, bindIp, () => {
        const addr = this.server.address();
        const actualPort = typeof addr === 'object' && addr ? addr.port : port;
        resolve({ host: bindIp, port: actualPort });
      });
    });
  }

  close(): Promise<void> {
    for (const c of this.clients) c.close();
    this.clients.clear();
    for (const a of this.liveAgents.values()) a.stop();
    this.liveAgents.clear();
    return new Promise((resolve) => this.server.close(() => resolve()));
  }

  private projects(): string[] {
    return discoverProjects(this.config);
  }

  private checkAuth(req: http.IncomingMessage, url: URL): boolean {
    const auth = req.headers.authorization;
    if (auth && auth.startsWith('Bearer ')) {
      return constantTimeEq(auth.slice(7).trim(), this.config.token);
    }
    const qTok = url.searchParams.get('token');
    if (qTok) {
      return constantTimeEq(qTok.trim(), this.config.token);
    }
    return false;
  }

  private buildSessionSummaries(): SessionSummary[] {
    const saved = scanSavedSessions(this.meta);
    const seen = new Set<string>();
    const list: SessionSummary[] = [];

    for (const s of saved) {
      seen.add(s.path);
      const m = this.meta[s.path];
      if (m?.hidden) continue;
      const live = this.liveAgents.get(s.path);
      const cwd = live?.cwd || s.cwd || this.projects()[0] || this.config.workspaces_dir;
      list.push({
        path: s.path,
        title: m?.title || live?.title || s.title,
        project: m?.repo_dir || cwd,
        project_name: projectNameForCwd(cwd, m),
        cwd,
        branch: m?.branch || live?.branch || null,
        status: live
          ? live.dialogs.length > 0
            ? 'Needs approval'
            : live.busy
              ? 'Working…'
              : 'Idle'
          : 'Saved',
        busy: Boolean(live?.busy),
        unread: false,
        has_pending_dialog: Boolean(live && live.dialogs.length > 0),
        model: live?.model || s.model,
        modified_ms: live ? Math.max(s.modified_ms, Date.now()) : s.modified_ms,
        created_ms: s.created_ms,
        origin: m?.origin || live?.origin || s.origin,
        parent_session: m?.parent_session || s.parent_session,
        pinned: Boolean(m?.pinned),
        is_active: false,
      });
    }

    for (const [p, live] of this.liveAgents.entries()) {
      if (seen.has(p)) continue;
      const m = this.meta[p];
      if (m?.hidden) continue;
      list.unshift({
        path: p,
        title: m?.title || live.title,
        project: m?.repo_dir || live.cwd,
        project_name: projectNameForCwd(live.cwd, m),
        cwd: live.cwd,
        branch: m?.branch || live.branch || null,
        status: live.dialogs.length > 0 ? 'Needs approval' : live.busy ? 'Working…' : 'Idle',
        busy: live.busy,
        unread: false,
        has_pending_dialog: live.dialogs.length > 0,
        model: live.model,
        modified_ms: Date.now(),
        created_ms: live.origin.created_at ?? Date.now(),
        origin: m?.origin || live.origin,
        parent_session: m?.parent_session ?? null,
        pinned: Boolean(m?.pinned),
        is_active: false,
      });
    }

    return list;
  }

  private broadcastSessions(): void {
    this.revision += 1;
    const payload = {
      type: 'sessions_updated',
      revision: this.revision,
      sessions: this.buildSessionSummaries(),
    };
    for (const c of this.clients) {
      c.sendJson(payload);
    }
  }

  private broadcastSessionDetail(agent: LiveAgent): void {
    const detail = agent.toDetail();
    const payload = {
      type: 'session_updated',
      revision: detail.revision,
      path: agent.sessionPath,
      detail,
    };
    for (const c of this.clients) {
      if (!c.subscribedPath || c.subscribedPath === agent.sessionPath) {
        c.sendJson(payload);
      }
    }
    this.broadcastSessions();
  }

  private broadcastNotification(note: NotificationEvent): void {
    const payload = { type: 'notification', notification: note };
    for (const c of this.clients) {
      c.sendJson(payload);
    }
  }

  private ensureLiveAgent(sessionPath: string, fallbackCwd?: string, permissionMode = 'auto'): LiveAgent {
    let agent = this.liveAgents.get(sessionPath);
    if (agent) return agent;

    let cwd = fallbackCwd || this.projects()[0] || this.config.workspaces_dir;
    let title = this.meta[sessionPath]?.title || 'Conversation';
    let origin = this.meta[sessionPath]?.origin || defaultCloudOrigin();
    if (fs.existsSync(sessionPath)) {
      try {
        const parsed = parseSessionFile(sessionPath, false);
        if (parsed.cwd && fs.existsSync(parsed.cwd)) cwd = parsed.cwd;
        if (!this.meta[sessionPath]?.title) title = parsed.title;
        if (!this.meta[sessionPath]?.origin) origin = parsed.origin;
      } catch {
        // ignore
      }
    }

    agent = new LiveAgent(
      sessionPath,
      cwd,
      title,
      origin,
      'auto',
      (changed) => this.broadcastSessionDetail(changed),
      (note) => this.broadcastNotification(note),
      this.meta[sessionPath]?.branch ?? null,
      this.meta[sessionPath]?.base_branch ?? null,
    );
    this.liveAgents.set(sessionPath, agent);
    agent.start();
    return agent;
  }

  private getSessionDetail(sessionPath: string): SessionDetail | null {
    const live = this.liveAgents.get(sessionPath);
    if (live) return live.toDetail();

    if (!fs.existsSync(sessionPath)) return null;
    try {
      const parsed = parseSessionFile(sessionPath, true);
      const m = this.meta[sessionPath];
      const cwd = parsed.cwd || this.projects()[0] || this.config.workspaces_dir;
      return {
        path: sessionPath,
        title: m?.title || parsed.title,
        cwd,
        project: cwd,
        status: 'Saved',
        busy: false,
        ready: true,
        model: parsed.model,
        model_name: parsed.model,
        provider: '',
        thinking: 'medium',
        levels: ['off', 'low', 'medium', 'high'],
        permission_mode: 'auto',
        cost: parsed.messages.reduce((acc, msg) => acc + (msg.cost || 0), 0),
        context_tokens: null,
        context_window: 200_000,
        error: null,
        notice: null,
        origin: m?.origin || parsed.origin,
        messages: parsed.messages,
        pending_messages: [],
        waiting_messages: [],
        dialogs: [],
        commands: LOCAL_SLASH_COMMANDS,
        models: [],
        artifacts: extractArtifacts(parsed.messages),
        revision: this.revision,
      };
    } catch {
      return null;
    }
  }

  private handleUpgrade(
    req: http.IncomingMessage,
    socket: import('node:stream').Duplex,
    head: Buffer,
  ): void {
    const url = new URL(req.url || '/', `http://localhost:${this.config.port}`);
    if (!this.checkAuth(req, url)) {
      socket.write('HTTP/1.1 401 Unauthorized\r\n\r\n');
      socket.destroy();
      return;
    }

    // Raw Pi RPC bridge for Pui Desktop cloud sessions (`/v1/rpc?path=...`).
    if (url.pathname === '/v1/rpc') {
      const sessionPath = url.searchParams.get('path') || '';
      if (!sessionPath || !fs.existsSync(sessionPath)) {
        socket.write('HTTP/1.1 404 Not Found\r\n\r\n');
        socket.destroy();
        return;
      }
      const perm = url.searchParams.get('permission_mode') || 'auto';
      const agent = this.ensureLiveAgent(sessionPath, undefined, perm);
      let unsubscribe: (() => void) | null = null;
      const rpcClient = acceptWebSocket(
        req,
        socket,
        head,
        (_c, msg) => {
          agent.writeRawRpc(msg);
        },
        () => {
          if (unsubscribe) unsubscribe();
          unsubscribe = null;
        },
      );
      if (!rpcClient) return;
      unsubscribe = agent.addRpcListener((rec) => {
        rpcClient.sendJson(rec);
      });
      return;
    }

    if (url.pathname !== '/v1/ws') {
      socket.write('HTTP/1.1 404 Not Found\r\n\r\n');
      socket.destroy();
      return;
    }

    const client = acceptWebSocket(
      req,
      socket,
      head,
      (c, msg) => {
        if (msg.type === 'ping') {
          c.sendJson({ type: 'pong', ts: Date.now() });
        } else if (msg.type === 'hello') {
          if (typeof msg.device === 'string') c.device = msg.device;
          if (typeof msg.platform === 'string') c.platform = msg.platform;
        } else if (msg.type === 'subscribe') {
          const p = typeof msg.path === 'string' && msg.path ? msg.path : null;
          c.subscribedPath = p;
          if (p) {
            const detail = this.getSessionDetail(p);
            if (detail) {
              c.sendJson({ type: 'session_updated', revision: detail.revision, path: p, detail });
            }
          }
        }
      },
      (c) => {
        this.clients.delete(c);
      },
    );

    if (!client) return;
    this.clients.add(client);
    client.sendJson({
      type: 'hello',
      protocol_version: 1,
      pui_version: '1.0.0-cloud',
      host_name: this.config.host_name,
      active_path: null,
    });
    client.sendJson({
      type: 'sessions_updated',
      revision: this.revision,
      sessions: this.buildSessionSummaries(),
    });
  }

  private async handleHttp(req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
    if (req.method === 'OPTIONS') {
      res.writeHead(204, {
        'Access-Control-Allow-Origin': '*',
        'Access-Control-Allow-Headers': 'Authorization, Content-Type',
        'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
      });
      res.end();
      return;
    }

    const url = new URL(req.url || '/', `http://localhost:${this.config.port}`);
    const method = (req.method || 'GET').toUpperCase();

    if (method === 'GET' && url.pathname === '/v1/health') {
      sendJson(res, 200, { ok: true, protocol_version: 1, host_type: 'cloud_daemon' });
      return;
    }

    if (!this.checkAuth(req, url)) {
      sendJson(res, 401, { error: 'Unauthorized' });
      return;
    }

    try {
      if (method === 'GET' && url.pathname === '/v1/info') {
        sendJson(res, 200, {
          protocol_version: 1,
          pui_version: '1.0.0-cloud',
          host_name: this.config.host_name,
          tailscale_ip: this.tailscaleIp,
          active_path: null,
          projects: this.projects(),
          catalog_projects: listCatalogProjects(this.config.workspaces_dir),
          permission_modes: PERMISSION_MODES,
          connected_devices: Array.from(this.clients).map((c) => ({
            id: c.id,
            platform: c.platform,
            device: c.device,
          })),
        });
        return;
      }

      if (method === 'GET' && url.pathname === '/v1/repos') {
        const q = url.searchParams.get('q') || undefined;
        const refresh = url.searchParams.get('refresh') === '1';
        sendJson(res, 200, {
          repos: listCloudRepos(this.config, { query: q, refresh }),
        });
        return;
      }

      if (method === 'GET' && url.pathname === '/v1/admin/status') {
        if (url.searchParams.get('refresh') === '1') invalidateGithubSshCheck();
        sendJson(res, 200, {
          ...getAdminStatus(this.config.workspaces_dir),
          projects: this.projects(),
          workspaces_dir: this.config.workspaces_dir,
        });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/admin/sync-projects') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const items = Array.isArray(body.projects)
          ? (body.projects as IncomingCatalogProject[])
          : [];
        const result = writeCatalogProjects(items, this.config.workspaces_dir);
        sendJson(res, 200, { ok: true, ...result });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/admin/sync-auth') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const auth = body.auth;
        if (!auth || typeof auth !== 'object' || Array.isArray(auth)) {
          sendJson(res, 400, { error: 'Missing auth object.' });
          return;
        }
        const merge = body.merge !== false;
        const result = writePiAuth(auth as Record<string, unknown>, merge);
        sendJson(res, 200, { ok: true, ...result });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/admin/sync-mcps') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const servers = body.mcpServers;
        if (!servers || typeof servers !== 'object' || Array.isArray(servers)) {
          sendJson(res, 400, { error: 'Missing mcpServers object.' });
          return;
        }
        const merge = body.merge !== false;
        const result = writeCloudMcps(servers as Record<string, unknown>, merge);
        sendJson(res, 200, { ok: true, ...result });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/admin/sync-skills') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const skills = Array.isArray(body.skills) ? (body.skills as IncomingSkillBundle[]) : [];
        const remove = Array.isArray(body.remove)
          ? body.remove.filter((x): x is string => typeof x === 'string')
          : [];
        const result = writeCloudSkills(skills, remove);
        sendJson(res, 200, { ok: true, ...result });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/admin/sync-service-creds') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const credentials = Array.isArray(body.credentials)
          ? (body.credentials as IncomingServiceCred[])
          : [];
        const remove = Array.isArray(body.remove)
          ? body.remove.filter((x): x is string => typeof x === 'string')
          : [];
        const result = writeCloudServiceCreds(credentials, remove);
        sendJson(res, 200, { ok: true, ...result });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/admin/git-setup') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        invalidateGithubSshCheck();
        if (body.remove === true) {
          const result = removeCloudGitKey();
          sendJson(res, 200, { ok: true, ...result });
          return;
        }
        const result = setupCloudGit({
          userName: typeof body.user_name === 'string' ? body.user_name : undefined,
          userEmail: typeof body.user_email === 'string' ? body.user_email : undefined,
          regenerateKey: Boolean(body.regenerate_key),
          privateKey: typeof body.private_key === 'string' ? body.private_key : undefined,
          publicKey: typeof body.public_key === 'string' ? body.public_key : undefined,
        });
        sendJson(res, 200, { ok: true, ...result });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/projects/clone') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const repoUrl = typeof body.url === 'string' ? body.url : '';
        if (!repoUrl.trim()) {
          sendJson(res, 400, { error: 'Missing repository url.' });
          return;
        }
        const result = cloneRepoIntoWorkspace(this.config, repoUrl, {
          name: typeof body.name === 'string' ? body.name : undefined,
          branch: typeof body.branch === 'string' ? body.branch : undefined,
        });
        sendJson(res, result.already_exists ? 200 : 201, {
          ok: true,
          ...result,
          projects: this.projects(),
        });
        return;
      }

      if (method === 'GET' && url.pathname === '/v1/usage') {
        sendJson(res, 200, {
          data: null,
          providers: [],
          unsupported: [],
          litellm_error: null,
        });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/usage/refresh') {
        sendJson(res, 202, { ok: true });
        return;
      }

      if (method === 'GET' && url.pathname === '/v1/models') {
        const live = Array.from(this.liveAgents.values()).find((a) => a.models.length > 0);
        if (live) {
          sendJson(res, 200, {
            models: live.models,
            current_model: live.model,
            current_provider: live.provider,
            current_thinking: live.thinking,
            levels: live.levels,
          });
          return;
        }
        sendJson(res, 200, {
          models: [],
          current_model: '',
          current_provider: '',
          current_thinking: 'medium',
          levels: ['off', 'low', 'medium', 'high'],
        });
        return;
      }

      if (method === 'GET' && url.pathname === '/v1/sessions') {
        sendJson(res, 200, {
          revision: this.revision,
          sessions: this.buildSessionSummaries(),
        });
        return;
      }

      if (method === 'GET' && url.pathname === '/v1/sessions/detail') {
        const target = url.searchParams.get('path');
        if (!target) {
          sendJson(res, 400, { error: 'Missing path query parameter.' });
          return;
        }
        const detail = this.getSessionDetail(target);
        if (!detail) {
          sendJson(res, 404, { error: 'Session not found.' });
          return;
        }
        sendJson(res, 200, detail);
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/sessions') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        let cwd: string | null = null;
        const catalog = listCatalogProjects(this.config.workspaces_dir);
        const catalogMatch =
          typeof body.cwd === 'string' && body.cwd.trim()
            ? catalog.find(
                (p) =>
                  p.name.toLowerCase() === body.cwd!.toString().trim().toLowerCase() ||
                  p.git_remote === body.cwd!.toString().trim(),
              )
            : undefined;
        const explicitRemote =
          typeof body.git_remote === 'string' && body.git_remote.trim()
            ? body.git_remote.trim()
            : catalogMatch?.git_remote ??
              (typeof body.cwd === 'string' && !fs.existsSync(body.cwd) && normalizeGitRemote(body.cwd)
                ? body.cwd.trim()
                : null);

        if (explicitRemote) {
          const cloned = cloneRepoIntoWorkspace(this.config, explicitRemote, {
            name: typeof body.repo_name === 'string' ? body.repo_name : catalogMatch?.name,
            branch: typeof body.branch === 'string' ? body.branch : undefined,
          });
          cwd = cloned.path;
        } else if (typeof body.cwd === 'string' && fs.existsSync(body.cwd)) {
          cwd = body.cwd;
        } else {
          cwd = this.projects()[0] || this.config.workspaces_dir;
        }

        // Isolate every remote cloud agent inside its own Git worktree & branch:
        // `pui_agent/<session-name-lowercased-hyphenated>-<6-random-chars>`
        const labelHint =
          (typeof body.title === 'string' && body.title.trim()) ||
          (typeof body.prompt === 'string' && body.prompt.trim()) ||
          (typeof body.repo_name === 'string' && body.repo_name.trim()) ||
          undefined;
        const wt = createCloudWorktree(this.config, cwd, labelHint);
        const sessionCwd = wt.cwd;

        const origin: SessionOrigin =
          body.origin && typeof body.origin === 'object'
            ? (body.origin as SessionOrigin)
            : { platform: 'android', device: 'Pui Mobile', client: 'pui-mobile', created_at: Date.now() };
        // Cloud agents always run in Auto mode.
        const perm = 'auto';
        const sessionPath = createEmptySessionFile(sessionCwd, origin);
        this.meta[sessionPath] = {
          origin,
          branch: wt.branch ?? undefined,
          base_branch: wt.baseBranch ?? undefined,
          repo_dir: wt.repoDir,
        };
        saveAllMeta(this.meta);

        const agent = this.ensureLiveAgent(sessionPath, sessionCwd, perm);
        const model = typeof body.model === 'string' && body.model.trim() ? body.model.trim() : undefined;
        const provider =
          typeof body.provider === 'string' && body.provider.trim() ? body.provider.trim() : undefined;
        const thinking =
          typeof body.thinking === 'string' && body.thinking.trim() ? body.thinking.trim() : undefined;
        if (model || thinking) {
          agent.updateSettings({ model, provider, thinking });
        }
        const images = Array.isArray(body.images)
          ? (body.images as { data_base64: string; mime: string; name?: string }[])
          : [];
        if (typeof body.prompt === 'string' && (body.prompt.trim() || images.length > 0)) {
          agent.sendPrompt(body.prompt.trim(), 'steer', images);
        }
        this.broadcastSessions();
        sendJson(res, 201, {
          ok: true,
          path: sessionPath,
          cwd: sessionCwd,
          branch: wt.branch,
          base_branch: wt.baseBranch,
        });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/sessions/open') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const target = typeof body.path === 'string' ? body.path : '';
        if (!target || !fs.existsSync(target)) {
          sendJson(res, 404, { error: 'Session not found.' });
          return;
        }
        this.ensureLiveAgent(target);
        sendJson(res, 202, { ok: true });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/sessions/send') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const target = typeof body.path === 'string' ? body.path : '';
        if (!target) {
          sendJson(res, 400, { error: 'Missing session path.' });
          return;
        }
        const text = typeof body.text === 'string' ? body.text : '';
        const mode = body.mode === 'steer' ? 'steer' : 'queue';
        const images = Array.isArray(body.images)
          ? (body.images as { data_base64: string; mime: string; name?: string }[])
          : [];
        const agent = this.ensureLiveAgent(target);
        agent.sendPrompt(text, mode, images);
        sendJson(res, 202, { ok: true });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/sessions/abort') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const target = typeof body.path === 'string' ? body.path : '';
        this.liveAgents.get(target)?.abort();
        sendJson(res, 202, { ok: true });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/sessions/dialog') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const target = typeof body.path === 'string' ? body.path : '';
        const dialogId = typeof body.dialog_id === 'string' ? body.dialog_id : '';
        const response =
          body.response && typeof body.response === 'object'
            ? (body.response as Record<string, unknown>)
            : {};
        const agent = target
          ? this.liveAgents.get(target)
          : Array.from(this.liveAgents.values()).find((a) =>
              a.dialogs.some((d) => d.id === dialogId),
            );
        agent?.answerDialog(dialogId, response);
        sendJson(res, 202, { ok: true });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/sessions/settings') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const target = typeof body.path === 'string' ? body.path : '';
        if (!target) {
          sendJson(res, 400, { error: 'Missing path.' });
          return;
        }
        const agent = this.ensureLiveAgent(target);
        agent.updateSettings({
          permission_mode: typeof body.permission_mode === 'string' ? body.permission_mode : undefined,
          model: typeof body.model === 'string' ? body.model : undefined,
          provider: typeof body.provider === 'string' ? body.provider : undefined,
          thinking: typeof body.thinking === 'string' ? body.thinking : undefined,
        });
        sendJson(res, 202, { ok: true });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/sessions/queue/remove') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const target = typeof body.path === 'string' ? body.path : '';
        const index = typeof body.index === 'number' ? body.index : -1;
        this.liveAgents.get(target)?.removeQueued(index);
        sendJson(res, 202, { ok: true });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/sessions/queue/steer') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const target = typeof body.path === 'string' ? body.path : '';
        const index = typeof body.index === 'number' ? body.index : -1;
        this.liveAgents.get(target)?.steerQueued(index);
        sendJson(res, 202, { ok: true });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/sessions/rename') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const target = typeof body.path === 'string' ? body.path : '';
        const title = typeof body.title === 'string' ? body.title.trim() : '';
        if (!target || !title) {
          sendJson(res, 400, { error: 'Missing path or title.' });
          return;
        }
        this.meta[target] = { ...(this.meta[target] ?? {}), title: title.slice(0, 120) };
        saveAllMeta(this.meta);
        const live = this.liveAgents.get(target);
        if (live) {
          live.title = title.slice(0, 120);
          live.sendRpc({ type: 'set_session_name', name: live.title });
        }
        this.broadcastSessions();
        sendJson(res, 202, { ok: true });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/sessions/pin') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const target = typeof body.path === 'string' ? body.path : '';
        const pinned = Boolean(body.pinned);
        if (!target) {
          sendJson(res, 400, { error: 'Missing path.' });
          return;
        }
        this.meta[target] = { ...(this.meta[target] ?? {}), pinned };
        saveAllMeta(this.meta);
        this.broadcastSessions();
        sendJson(res, 202, { ok: true });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/sessions/delete') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const target = typeof body.path === 'string' ? body.path : '';
        if (!target) {
          sendJson(res, 400, { error: 'Missing path.' });
          return;
        }
        const live = this.liveAgents.get(target);
        if (live) {
          live.stop();
          this.liveAgents.delete(target);
        }
        if (fs.existsSync(target) && target.endsWith('.jsonl')) {
          try {
            fs.unlinkSync(target);
          } catch {
            // ignore
          }
        }
        delete this.meta[target];
        saveAllMeta(this.meta);
        this.broadcastSessions();
        sendJson(res, 202, { ok: true });
        return;
      }

      if (method === 'POST' && url.pathname === '/v1/sessions/fork') {
        const body = JSON.parse((await readBody(req)).toString('utf8') || '{}') as Record<
          string,
          unknown
        >;
        const source = typeof body.path === 'string' ? body.path : '';
        if (!source || !fs.existsSync(source)) {
          sendJson(res, 404, { error: 'Source session not found.' });
          return;
        }
        const origin: SessionOrigin =
          body.origin && typeof body.origin === 'object'
            ? (body.origin as SessionOrigin)
            : defaultCloudOrigin();
        const target = forkSessionFile(source, origin);
        const sourceTitle = this.meta[source]?.title || parseSessionFile(source, false).title;
        this.meta[target] = {
          title: `${sourceTitle} (fork)`,
          origin,
          parent_session: source,
        };
        saveAllMeta(this.meta);
        this.ensureLiveAgent(target);
        this.broadcastSessions();
        sendJson(res, 201, { ok: true, path: target });
        return;
      }

      if (method === 'GET' && url.pathname === '/v1/completions/commands') {
        const q = (url.searchParams.get('q') || '').trim().toLowerCase();
        const target = url.searchParams.get('path') || '';
        const live = target ? this.liveAgents.get(target) : undefined;
        const combined = [...LOCAL_SLASH_COMMANDS, ...(live?.commands ?? [])];
        const filtered = combined.filter(
          (c) => !q || c.name.toLowerCase().includes(q) || c.description.toLowerCase().includes(q),
        );
        sendJson(res, 200, { commands: filtered });
        return;
      }

      if (method === 'GET' && url.pathname === '/v1/completions/files') {
        const cwd = url.searchParams.get('cwd') || this.projects()[0] || this.config.workspaces_dir;
        const q = url.searchParams.get('q') || '';
        const files = searchWorkspaceFiles(cwd, q, this.projects());
        sendJson(res, 200, { files });
        return;
      }

      if (method === 'GET' && url.pathname === '/v1/files/tree') {
        const cwd = url.searchParams.get('cwd') || this.projects()[0] || this.config.workspaces_dir;
        const dir = url.searchParams.get('dir') || '';
        sendJson(res, 200, listFileTree(cwd, dir, this.projects()));
        return;
      }

      if (method === 'GET' && url.pathname === '/v1/files/content') {
        const cwd = url.searchParams.get('cwd') || this.projects()[0] || this.config.workspaces_dir;
        const filePath = url.searchParams.get('path') || '';
        sendJson(res, 200, readFileContent(cwd, filePath, this.projects()));
        return;
      }

      if (method === 'GET' && url.pathname === '/v1/files/raw') {
        const cwd = url.searchParams.get('cwd') || this.projects()[0] || this.config.workspaces_dir;
        const filePath = url.searchParams.get('path') || '';
        const resolved = resolveSafeFile(cwd, filePath, this.projects());
        if (!resolved) {
          sendJson(res, 404, { error: 'File not found.' });
          return;
        }
        res.writeHead(200, {
          'Content-Type': mimeForPath(resolved.fullPath),
          'Cache-Control': 'no-store',
          'Access-Control-Allow-Origin': '*',
        });
        fs.createReadStream(resolved.fullPath).pipe(res);
        return;
      }

      if (method === 'GET' && url.pathname === '/v1/git/changes') {
        const cwd = url.searchParams.get('cwd') || this.projects()[0] || this.config.workspaces_dir;
        sendJson(res, 200, gitChanges(cwd, this.projects()));
        return;
      }

      if (method === 'GET' && url.pathname === '/v1/git/diff') {
        const cwd = url.searchParams.get('cwd') || this.projects()[0] || this.config.workspaces_dir;
        const filePath = url.searchParams.get('path') || '';
        const status = url.searchParams.get('status') || ' M';
        sendJson(res, 200, { path: filePath, diff: gitDiff(cwd, filePath, status, this.projects()) });
        return;
      }

      sendJson(res, 404, { error: 'Unknown endpoint.' });
    } catch (err: unknown) {
      // Only honour explicit HTTP error codes; child-process errors carry exit codes (e.g. 128) in `status`.
      const raw = (err as { status?: unknown })?.status;
      const status = typeof raw === 'number' && raw >= 400 && raw <= 599 ? raw : 500;
      sendJson(res, status, {
        error: err instanceof Error ? err.message : 'Internal server error',
      });
    }
  }
}
