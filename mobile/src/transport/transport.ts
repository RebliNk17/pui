import type {
  FileContentResponse,
  FileMentionItem,
  GitChangesResponse,
  HostInfo,
  SavedHost,
  SessionDetail,
  SessionOrigin,
  SessionSummary,
  SlashCommandItem,
  TreeResponse,
  WsServerEvent,
} from '../protocol/types';
import { buildBaseHttpUrl, buildWsUrl } from '../utils/pairing';

export type ConnectionState = 'disconnected' | 'connecting' | 'connected' | 'error';

export interface TransportCallbacks {
  onStateChange: (state: ConnectionState, error?: string) => void;
  onEvent: (event: WsServerEvent) => void;
}

/**
 * Transport-agnostic client interface.
 * Phase 1 uses `DirectTransport` (Tailscale / LAN HTTP + WebSocket).
 * Phase 2 swaps in `CloudRelayTransport` (E2EE relay) using the exact same contract.
 */
export interface PuiTransport {
  connect(deviceName: string, platform: 'android' | 'ios'): void;
  disconnect(): void;
  subscribeSession(path: string | null): void;
  getInfo(): Promise<HostInfo>;
  listSessions(): Promise<{ revision: number; active_path: string | null; sessions: SessionSummary[] }>;
  getSessionDetail(path?: string): Promise<SessionDetail>;
  createSession(input: {
    cwd: string;
    permission_mode?: string;
    prompt?: string;
    origin: SessionOrigin;
  }): Promise<{ ok: boolean; origin: SessionOrigin }>;
  openSession(path: string): Promise<void>;
  sendMessage(input: {
    path?: string;
    text: string;
    mode?: 'steer' | 'queue';
    images?: { data_base64: string; mime: string; name?: string }[];
    origin?: SessionOrigin;
  }): Promise<void>;
  abortSession(path?: string): Promise<void>;
  answerDialog(input: {
    path?: string;
    dialog_id: string;
    response: Record<string, unknown>;
  }): Promise<void>;
  updateSessionSettings(input: {
    path?: string;
    permission_mode?: string;
    model?: string;
    provider?: string;
    thinking?: string;
  }): Promise<void>;
  getSlashCommands(path?: string, query?: string): Promise<SlashCommandItem[]>;
  getFileMentions(cwd: string, query: string): Promise<FileMentionItem[]>;
  getFileTree(cwd: string, dir?: string): Promise<TreeResponse>;
  getFileContent(cwd: string, path: string): Promise<FileContentResponse>;
  getRawFileUrl(cwd: string, path: string): string;
  getGitChanges(cwd: string): Promise<GitChangesResponse>;
  getGitDiff(cwd: string, path: string, status?: string): Promise<string>;
}

/**
 * Phase 1: Direct HTTP + WebSocket transport over Tailscale (or LAN / tailscale serve).
 */
export class DirectTransport implements PuiTransport {
  private readonly host: SavedHost;
  private readonly callbacks: TransportCallbacks;
  private ws: WebSocket | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private pingTimer: ReturnType<typeof setInterval> | null = null;
  private closedManually = false;
  private attempt = 0;
  private subscribedPath: string | null = null;
  private clientMeta: { device: string; platform: 'android' | 'ios' } = {
    device: 'Phone',
    platform: 'android',
  };

  constructor(host: SavedHost, callbacks: TransportCallbacks) {
    this.host = host;
    this.callbacks = callbacks;
  }

  connect(deviceName: string, platform: 'android' | 'ios'): void {
    this.clientMeta = { device: deviceName, platform };
    this.closedManually = false;
    this.openSocket();
  }

  disconnect(): void {
    this.closedManually = true;
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.pingTimer) {
      clearInterval(this.pingTimer);
      this.pingTimer = null;
    }
    if (this.ws) {
      try {
        this.ws.close();
      } catch {
        // ignore
      }
      this.ws = null;
    }
    this.callbacks.onStateChange('disconnected');
  }

  subscribeSession(path: string | null): void {
    this.subscribedPath = path;
    if (this.ws && this.ws.readyState === WebSocket.OPEN && path) {
      this.ws.send(JSON.stringify({ type: 'subscribe', path }));
    }
  }

  private openSocket(): void {
    if (this.closedManually) return;
    this.callbacks.onStateChange('connecting');
    const url = buildWsUrl(this.host);
    const socket = new WebSocket(url);
    this.ws = socket;

    socket.onopen = () => {
      this.attempt = 0;
      this.callbacks.onStateChange('connected');
      socket.send(
        JSON.stringify({
          type: 'client_hello',
          platform: this.clientMeta.platform,
          device: this.clientMeta.device,
          client: 'pui-mobile',
        }),
      );
      if (this.subscribedPath) {
        socket.send(JSON.stringify({ type: 'subscribe', path: this.subscribedPath }));
      }
      if (this.pingTimer) clearInterval(this.pingTimer);
      this.pingTimer = setInterval(() => {
        if (socket.readyState === WebSocket.OPEN) {
          socket.send(JSON.stringify({ type: 'ping', ts: Date.now() }));
        }
      }, 25_000);
    };

    socket.onmessage = (event) => {
      try {
        const parsed = JSON.parse(String(event.data)) as WsServerEvent;
        this.callbacks.onEvent(parsed);
      } catch {
        // Ignore malformed server frame.
      }
    };

    socket.onerror = () => {
      this.callbacks.onStateChange('error', 'Could not reach Pui host. Check Tailscale and token.');
    };

    socket.onclose = () => {
      if (this.pingTimer) {
        clearInterval(this.pingTimer);
        this.pingTimer = null;
      }
      if (this.closedManually) {
        this.callbacks.onStateChange('disconnected');
        return;
      }
      this.callbacks.onStateChange('disconnected');
      const delay = Math.min(1000 * 2 ** this.attempt, 15_000);
      this.attempt += 1;
      this.reconnectTimer = setTimeout(() => this.openSocket(), delay);
    };
  }

  private async requestJson<T>(path: string, init?: RequestInit): Promise<T> {
    const base = buildBaseHttpUrl(this.host);
    const res = await fetch(`${base}${path}`, {
      ...init,
      headers: {
        Authorization: `Bearer ${this.host.token}`,
        'Content-Type': 'application/json',
        ...(init?.headers ?? {}),
      },
    });
    if (!res.ok) {
      let detail = `HTTP ${res.status}`;
      try {
        const body = (await res.json()) as { error?: string };
        if (body.error) detail = body.error;
      } catch {
        // ignore
      }
      throw new Error(detail);
    }
    return (await res.json()) as T;
  }

  getInfo(): Promise<HostInfo> {
    return this.requestJson<HostInfo>('/v1/info');
  }

  listSessions(): Promise<{ revision: number; active_path: string | null; sessions: SessionSummary[] }> {
    return this.requestJson('/v1/sessions');
  }

  getSessionDetail(path?: string): Promise<SessionDetail> {
    const q = path ? `?path=${encodeURIComponent(path)}` : '';
    return this.requestJson<SessionDetail>(`/v1/sessions/detail${q}`);
  }

  createSession(input: {
    cwd: string;
    permission_mode?: string;
    prompt?: string;
    origin: SessionOrigin;
  }): Promise<{ ok: boolean; origin: SessionOrigin }> {
    return this.requestJson('/v1/sessions', {
      method: 'POST',
      body: JSON.stringify(input),
    });
  }

  async openSession(path: string): Promise<void> {
    await this.requestJson('/v1/sessions/open', {
      method: 'POST',
      body: JSON.stringify({ path }),
    });
  }

  async sendMessage(input: {
    path?: string;
    text: string;
    mode?: 'steer' | 'queue';
    images?: { data_base64: string; mime: string; name?: string }[];
    origin?: SessionOrigin;
  }): Promise<void> {
    await this.requestJson('/v1/sessions/message', {
      method: 'POST',
      body: JSON.stringify(input),
    });
  }

  async abortSession(path?: string): Promise<void> {
    await this.requestJson('/v1/sessions/abort', {
      method: 'POST',
      body: JSON.stringify({ path }),
    });
  }

  async answerDialog(input: {
    path?: string;
    dialog_id: string;
    response: Record<string, unknown>;
  }): Promise<void> {
    await this.requestJson('/v1/sessions/dialog', {
      method: 'POST',
      body: JSON.stringify(input),
    });
  }

  async updateSessionSettings(input: {
    path?: string;
    permission_mode?: string;
    model?: string;
    provider?: string;
    thinking?: string;
  }): Promise<void> {
    await this.requestJson('/v1/sessions/settings', {
      method: 'POST',
      body: JSON.stringify(input),
    });
  }

  async getSlashCommands(path?: string, query = ''): Promise<SlashCommandItem[]> {
    const params = new URLSearchParams();
    if (path) params.set('path', path);
    if (query) params.set('q', query);
    const res = await this.requestJson<{ commands: SlashCommandItem[] }>(
      `/v1/completions/commands?${params.toString()}`,
    );
    return res.commands ?? [];
  }

  async getFileMentions(cwd: string, query: string): Promise<FileMentionItem[]> {
    const params = new URLSearchParams({ cwd, q: query });
    const res = await this.requestJson<{ files: FileMentionItem[] }>(
      `/v1/completions/files?${params.toString()}`,
    );
    return res.files ?? [];
  }

  getFileTree(cwd: string, dir = ''): Promise<TreeResponse> {
    const params = new URLSearchParams({ cwd, dir });
    return this.requestJson<TreeResponse>(`/v1/files/tree?${params.toString()}`);
  }

  getFileContent(cwd: string, path: string): Promise<FileContentResponse> {
    const params = new URLSearchParams({ cwd, path });
    return this.requestJson<FileContentResponse>(`/v1/files/content?${params.toString()}`);
  }

  getRawFileUrl(cwd: string, path: string): string {
    const base = buildBaseHttpUrl(this.host);
    const params = new URLSearchParams({ cwd, path, token: this.host.token });
    return `${base}/v1/files/raw?${params.toString()}`;
  }

  getGitChanges(cwd: string): Promise<GitChangesResponse> {
    const params = new URLSearchParams({ cwd });
    return this.requestJson<GitChangesResponse>(`/v1/git/changes?${params.toString()}`);
  }

  async getGitDiff(cwd: string, path: string, status = ' M'): Promise<string> {
    const params = new URLSearchParams({ cwd, path, status });
    const res = await this.requestJson<{ diff: string }>(`/v1/git/diff?${params.toString()}`);
    return res.diff ?? '';
  }
}

/**
 * Phase 2 placeholder: End-to-end encrypted Cloud Relay transport.
 * Reuses the exact same `PuiTransport` interface & v1 protocol messages, wrapped in
 * X25519 + ChaCha20-Poly1305 frames routed through a blind relay server.
 */
export class CloudRelayTransport extends DirectTransport {
  constructor(host: SavedHost, callbacks: TransportCallbacks) {
    super(host, callbacks);
  }
}
