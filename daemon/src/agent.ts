import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import {
  extractArtifacts,
  parsePiContentToMessage,
  parseSessionFile,
  type ArtifactItem,
  type MessageRecord,
  type SessionOrigin,
  type ToolRecord,
} from './sessions.ts';

export interface WaitingItem {
  index: number;
  text: string;
  mode: string;
  image_count: number;
}

export interface QueuedPayload {
  text: string;
  mode: 'queue' | 'steer';
  images: { data_base64: string; mime: string; name?: string }[];
}

export interface NotificationEvent {
  id: string;
  kind: 'finished' | 'approval' | 'question';
  path: string;
  title: string;
  body: string;
  timestamp: number;
  dialog_id?: string;
  dialog_method?: string;
}

export interface SessionDetail {
  path: string;
  title: string;
  cwd: string;
  project: string;
  status: string;
  busy: boolean;
  ready: boolean;
  model: string;
  model_name: string;
  provider: string;
  thinking: string;
  levels: string[];
  permission_mode: string;
  cost: number;
  context_tokens: number | null;
  context_window: number;
  error: string | null;
  notice: string | null;
  origin: SessionOrigin;
  messages: MessageRecord[];
  pending_messages: unknown[];
  waiting_messages: WaitingItem[];
  dialogs: Record<string, unknown>[];
  commands: { name: string; description: string; source: string }[];
  models: { id: string; name: string; provider: string; context_window?: number }[];
  artifacts: ArtifactItem[];
  revision: number;
}

function resolvePiCliPath(): string {
  const candidates = [
    path.resolve(import.meta.dirname, '../node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js'),
    path.resolve(import.meta.dirname, '../../packaging/runtime/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js'),
  ];
  for (const c of candidates) {
    if (fs.existsSync(c)) return c;
  }
  throw new Error('Pi CLI not found. Run `npm install` inside daemon/.');
}

function resolveAdapterPath(): string | null {
  const candidate = path.resolve(import.meta.dirname, '../../scripts/ultracode.ts');
  return fs.existsSync(candidate) ? candidate : null;
}

function resolveDaemonNodeModules(): string | null {
  const candidate = path.resolve(import.meta.dirname, '../node_modules');
  return fs.existsSync(candidate) ? candidate : null;
}

function resolveCloudWorkflowPath(): string | null {
  const candidate = path.resolve(import.meta.dirname, './cloud-workflow.ts');
  return fs.existsSync(candidate) ? candidate : null;
}

function truncateText(text: string, max = 140): string {
  const clean = text.replace(/\s+/g, ' ').trim();
  return clean.length <= max ? clean : `${clean.slice(0, max - 1)}…`;
}

export class LiveAgent {
  readonly sessionPath: string;
  readonly cwd: string;
  readonly branch: string | null;
  readonly baseBranch: string | null;
  private proc: ChildProcessWithoutNullStreams | null = null;
  private reqSeq = 0;
  private onChange: (agent: LiveAgent) => void;
  private onNotify: (event: NotificationEvent) => void;
  private rpcListeners = new Set<(record: Record<string, unknown>) => void>();
  private seenDialogIds = new Set<string>();
  private queued: QueuedPayload[] = [];
  private streamingMsg: MessageRecord | null = null;
  private toolById = new Map<string, ToolRecord>();
  private stderrBuf = '';

  title: string;
  origin: SessionOrigin;
  busy = false;
  ready = false;
  model = '';
  modelName = '';
  provider = '';
  thinking = 'medium';
  levels: string[] = ['off', 'low', 'medium', 'high'];
  permissionMode = 'auto';
  cost = 0;
  contextTokens: number | null = null;
  contextWindow = 200_000;
  error: string | null = null;
  notice: string | null = null;
  messages: MessageRecord[] = [];
  dialogs: Record<string, unknown>[] = [];
  commands: { name: string; description: string; source: string }[] = [];
  models: { id: string; name: string; provider: string; context_window?: number }[] = [];
  revision = 1;

  constructor(
    sessionPath: string,
    cwd: string,
    title: string,
    origin: SessionOrigin,
    _permissionMode: string,
    onChange: (agent: LiveAgent) => void,
    onNotify: (event: NotificationEvent) => void,
    branch: string | null = null,
    baseBranch: string | null = null,
  ) {
    this.sessionPath = sessionPath;
    this.cwd = cwd;
    this.title = title;
    this.origin = origin;
    // Cloud agents always run in Auto mode inside their isolated worktree.
    this.permissionMode = 'auto';
    this.branch = branch;
    this.baseBranch = baseBranch;
    this.onChange = onChange;
    this.onNotify = onNotify;

    // Immediately hydrate existing messages from disk so mobile never sees a blank frame.
    if (fs.existsSync(sessionPath)) {
      try {
        const parsed = parseSessionFile(sessionPath, true);
        this.messages = parsed.messages;
        if (!this.title || this.title === 'New conversation') this.title = parsed.title;
        if (parsed.model) this.model = parsed.model;
        for (const m of this.messages) {
          this.cost += m.cost || 0;
          for (const t of m.tools) this.toolById.set(t.id, t);
        }
      } catch {
        // ignore
      }
    }
  }

  start(): void {
    if (this.proc) return;
    const cli = resolvePiCliPath();
    const adapter = resolveAdapterPath();
    const workflowExt = resolveCloudWorkflowPath();
    const args = [cli, '--mode', 'rpc', '--session', this.sessionPath];
    // `scripts/ultracode.ts` already installs `scripts/permissions.ts` internally;
    // do not pass `permissions.ts` directly (it is a helper module, not a Pi extension).
    if (adapter) {
      args.push('--extension', adapter);
    }
    if (workflowExt && this.branch) {
      args.push('--extension', workflowExt);
    }

    const daemonNodeModules = resolveDaemonNodeModules();
    const childEnv: NodeJS.ProcessEnv = {
      ...process.env,
      PI_SKIP_VERSION_CHECK: '1',
      PUI_AGENT_ACCESS: 'worktree',
      PUI_SETTINGS: JSON.stringify({ permission: 'auto' }),
    };
    if (daemonNodeModules) {
      childEnv.NODE_PATH = childEnv.NODE_PATH
        ? `${daemonNodeModules}:${childEnv.NODE_PATH}`
        : daemonNodeModules;
      const sdkDir = path.join(daemonNodeModules, '@earendil-works', 'pi-coding-agent');
      if (fs.existsSync(sdkDir)) {
        childEnv.PUI_PI_SDK = sdkDir;
      }
    }
    if (this.branch) {
      childEnv.PUI_CLOUD_BRANCH = this.branch;
      childEnv.PUI_CLOUD_BASE_BRANCH = this.baseBranch || 'main';
      childEnv.PUI_CLOUD_WORKTREE = this.cwd;
    }

    this.stderrBuf = '';
    const child = spawn(process.execPath, args, {
      cwd: this.cwd,
      env: childEnv,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    this.proc = child;

    const rl = readline.createInterface({ input: child.stdout });
    rl.on('line', (line) => {
      if (!line.trim()) return;
      try {
        const rec = JSON.parse(line) as Record<string, unknown>;
        for (const listener of this.rpcListeners) {
          try {
            listener(rec);
          } catch {
            // ignore
          }
        }
        this.handleRecord(rec);
      } catch {
        // ignore non-JSON output
      }
    });

    child.stderr.on('data', (chunk: Buffer) => {
      const text = chunk.toString('utf8').trim();
      if (text && !text.includes('ExperimentalWarning')) {
        this.stderrBuf = `${this.stderrBuf}\n${text}`.trim().slice(-1200);
        console.error(`[pui-daemon agent ${path.basename(this.sessionPath)}] ${text}`);
        this.notice = text.slice(0, 300);
      }
    });

    child.on('exit', (code, signal) => {
      this.proc = null;
      this.busy = false;
      this.ready = false;
      if (code && code !== 0) {
        const detail = this.stderrBuf || `Pi exited with code ${code}${signal ? ` (${signal})` : ''}`;
        this.error = detail.slice(0, 600);
        console.error(`[pui-daemon agent exit ${code}] ${detail}`);
      }
      this.bump();
    });

    this.sendRpc({ type: 'get_state' });
    this.sendRpc({ type: 'get_available_models' });
    this.sendRpc({ type: 'get_commands' });
    this.ready = true;
    this.bump();
  }

  stop(): void {
    if (this.proc) {
      try {
        this.proc.kill('SIGTERM');
      } catch {
        // ignore
      }
      this.proc = null;
    }
  }

  private bump(): void {
    this.revision += 1;
    this.onChange(this);
  }

  addRpcListener(listener: (record: Record<string, unknown>) => void): () => void {
    this.rpcListeners.add(listener);
    return () => {
      this.rpcListeners.delete(listener);
    };
  }

  writeRawRpc(record: Record<string, unknown>): void {
    if (!this.proc) this.start();
    if (!this.proc?.stdin.writable) return;
    try {
      this.proc.stdin.write(JSON.stringify(record) + '\n');
    } catch {
      // ignore
    }
  }

  sendRpc(payload: Record<string, unknown>): void {
    if (!this.proc) this.start();
    if (!this.proc?.stdin.writable) return;
    const id = `req-${++this.reqSeq}`;
    try {
      this.proc.stdin.write(JSON.stringify({ id, ...payload }) + '\n');
    } catch {
      // ignore
    }
  }

  sendPrompt(
    text: string,
    mode: 'queue' | 'steer',
    images: { data_base64: string; mime: string; name?: string }[] = [],
  ): void {
    if (!this.proc) this.start();
    if (this.busy) {
      if (mode === 'steer') {
        this.deliverNow(text, 'steer', images);
      } else {
        this.queued.push({ text, mode: 'queue', images });
        this.bump();
      }
      return;
    }
    this.deliverNow(text, 'prompt', images);
  }

  private deliverNow(
    text: string,
    type: 'prompt' | 'steer',
    images: { data_base64: string; mime: string; name?: string }[],
  ): void {
    this.error = null;
    this.busy = true;
    if (this.messages.length === 0 && text.trim()) {
      this.title = text.trim().split('\n')[0].slice(0, 90);
    }
    const payload: Record<string, unknown> = { type, message: text };
    if (images.length > 0) {
      payload.images = images.map((img) => ({
        type: 'image',
        data: img.data_base64,
        mimeType: img.mime || 'image/jpeg',
      }));
    }
    this.sendRpc(payload);
    this.bump();
  }

  removeQueued(index: number): void {
    if (index >= 0 && index < this.queued.length) {
      this.queued.splice(index, 1);
      this.bump();
    }
  }

  steerQueued(index: number): void {
    if (index >= 0 && index < this.queued.length) {
      const [item] = this.queued.splice(index, 1);
      this.deliverNow(item.text, 'steer', item.images);
    }
  }

  abort(): void {
    this.queued = [];
    this.sendRpc({ type: 'abort' });
    this.bump();
  }

  answerDialog(dialogId: string, response: Record<string, unknown>): void {
    this.dialogs = this.dialogs.filter((d) => d.id !== dialogId);
    this.sendRpc({
      type: 'extension_ui_response',
      id: dialogId,
      ...response,
    });
    this.bump();
  }

  updateSettings(opts: {
    permission_mode?: string;
    model?: string;
    provider?: string;
    thinking?: string;
  }): void {
    // Cloud agents always stay in Auto mode.
    if (opts.permission_mode) {
      this.permissionMode = 'auto';
    }
    if (opts.model) {
      const prov = opts.provider || this.provider;
      if (prov) {
        this.model = opts.model;
        this.provider = prov;
        this.sendRpc({ type: 'set_model', provider: prov, modelId: opts.model });
      }
    }
    if (opts.thinking) {
      this.thinking = opts.thinking;
      this.sendRpc({ type: 'set_thinking_level', level: opts.thinking });
    }
    this.bump();
  }

  private handleRecord(rec: Record<string, unknown>): void {
    const type = typeof rec.type === 'string' ? rec.type : '';

    if (type === 'response' && rec.success === false) {
      const cmd = typeof rec.command === 'string' ? rec.command : '';
      const errMsg = typeof rec.error === 'string' && rec.error.trim() ? rec.error.trim() : `${cmd || 'Command'} failed.`;
      this.error = errMsg;
      if (cmd === 'prompt' || cmd === 'steer') {
        this.busy = false;
      }
      this.bump();
      return;
    }

    if (type === 'response' && rec.success === true) {
      const cmd = rec.command;
      const data =
        rec.data && typeof rec.data === 'object' ? (rec.data as Record<string, unknown>) : null;
      if (cmd === 'get_state' && data) {
        this.busy = Boolean(data.isStreaming);
        if (typeof data.thinkingLevel === 'string') this.thinking = data.thinkingLevel;
        if (typeof data.sessionName === 'string' && data.sessionName.trim()) {
          this.title = data.sessionName.trim();
        }
        const m = data.model as Record<string, unknown> | undefined;
        if (m) {
          if (typeof m.id === 'string') this.model = m.id;
          if (typeof m.name === 'string') this.modelName = m.name;
          if (typeof m.provider === 'string') this.provider = m.provider;
          if (typeof m.contextWindow === 'number') this.contextWindow = m.contextWindow;
        }
        this.bump();
      } else if (cmd === 'get_available_models' && data && Array.isArray(data.models)) {
        this.models = (data.models as Record<string, unknown>[]).map((m) => ({
          id: String(m.id ?? ''),
          name: String(m.name ?? m.id ?? ''),
          provider: String(m.provider ?? ''),
          context_window: typeof m.contextWindow === 'number' ? m.contextWindow : undefined,
        }));
        this.bump();
      } else if (cmd === 'get_commands' && data && Array.isArray(data.commands)) {
        this.commands = (data.commands as Record<string, unknown>[]).map((c) => ({
          name: String(c.name ?? ''),
          description: String(c.description ?? ''),
          source: String(c.source ?? 'pi'),
        }));
        this.bump();
      }
      return;
    }

    if (type === 'agent_start') {
      this.busy = true;
      this.error = null;
      this.bump();
      return;
    }

    if (type === 'agent_end') {
      const wasBusy = this.busy;
      this.busy = false;
      this.streamingMsg = null;
      this.reloadHistoryFromDisk();
      this.bump();

      // Deliver next queued message if any, else notify user that agent finished!
      if (this.queued.length > 0 && this.dialogs.length === 0) {
        const next = this.queued.shift()!;
        this.deliverNow(next.text, 'prompt', next.images);
      } else if (wasBusy && this.dialogs.length === 0) {
        const lastAssistant = [...this.messages]
          .reverse()
          .find((m) => m.role === 'assistant' && m.text.trim());
        this.onNotify({
          id: `finished:${this.sessionPath}:${Date.now()}`,
          kind: 'finished',
          path: this.sessionPath,
          title: this.title || 'pui-cloud',
          body: truncateText(lastAssistant?.text || 'Agent finished replying.'),
          timestamp: Date.now(),
        });
      }
      return;
    }

    if (type === 'message_start' && rec.message && typeof rec.message === 'object') {
      const parsed = parsePiContentToMessage(rec.message as Record<string, unknown>, this.revision);
      if (parsed.role === 'user') {
        this.messages.push(parsed);
      } else if (parsed.role === 'assistant') {
        this.streamingMsg = parsed;
        this.messages.push(parsed);
      }
      this.bump();
      return;
    }

    if (type === 'message_update' && rec.assistantMessageEvent && typeof rec.assistantMessageEvent === 'object') {
      const ev = rec.assistantMessageEvent as Record<string, unknown>;
      if (!this.streamingMsg) {
        this.streamingMsg = {
          role: 'assistant',
          text: '',
          thinking: '',
          provider: this.provider,
          model: this.model,
          tools: [],
          images: [],
          timestamp: Date.now(),
          cost: 0,
          effort: this.thinking,
          revision: this.revision,
        };
        this.messages.push(this.streamingMsg);
      }
      if (ev.type === 'text_delta' && typeof ev.delta === 'string') {
        this.streamingMsg.text += ev.delta;
        this.streamingMsg.revision += 1;
        this.bump();
      } else if (ev.type === 'thinking_delta' && typeof ev.delta === 'string') {
        this.streamingMsg.thinking += ev.delta;
        this.streamingMsg.revision += 1;
        this.bump();
      }
      return;
    }

    if (type === 'message_end' && rec.message && typeof rec.message === 'object') {
      const raw = rec.message as Record<string, unknown>;
      const parsed = parsePiContentToMessage(raw, this.revision);
      if (parsed.role === 'assistant') {
        if (this.streamingMsg) {
          const idx = this.messages.lastIndexOf(this.streamingMsg);
          if (idx >= 0) this.messages[idx] = parsed;
          else this.messages.push(parsed);
          this.streamingMsg = null;
        } else {
          this.messages.push(parsed);
        }
        for (const t of parsed.tools) this.toolById.set(t.id, t);
        if (parsed.cost > 0) this.cost += parsed.cost;
        if (parsed.error && parsed.error.trim()) {
          // Strip multi-line Node stack trace from provider error messages for clean display.
          const firstLine = parsed.error.split('\n')[0]?.trim() || parsed.error.trim();
          this.error = firstLine.slice(0, 400);
        }
        const usage = raw.usage as Record<string, unknown> | undefined;
        if (usage && typeof usage.totalTokens === 'number') {
          this.contextTokens = usage.totalTokens;
        }
        this.bump();
      }
      return;
    }

    if (type === 'tool_execution_start') {
      const id = String(rec.toolCallId ?? crypto.randomUUID());
      const name = String(rec.toolName ?? 'tool');
      const argsObj = (rec.args && typeof rec.args === 'object'
        ? rec.args
        : {}) as Record<string, unknown>;
      let tool = this.toolById.get(id);
      if (!tool) {
        tool = {
          id,
          name,
          args: JSON.stringify(argsObj),
          output: '',
          failed: false,
          finished: false,
          path: typeof argsObj.path === 'string' ? argsObj.path : '',
          command: typeof argsObj.command === 'string' ? argsObj.command : '',
          diff: '',
          added: 0,
          removed: 0,
          code: typeof argsObj.code === 'string' ? argsObj.code : '',
          children: [],
          images: [],
        };
        this.toolById.set(id, tool);
        if (!this.streamingMsg) {
          this.streamingMsg = {
            role: 'assistant',
            text: '',
            thinking: '',
            provider: this.provider,
            model: this.model,
            tools: [tool],
            images: [],
            timestamp: Date.now(),
            cost: 0,
            effort: this.thinking,
            revision: this.revision,
          };
          this.messages.push(this.streamingMsg);
        } else {
          this.streamingMsg.tools.push(tool);
        }
      }
      this.bump();
      return;
    }

    if (type === 'tool_execution_end') {
      const id = String(rec.toolCallId ?? '');
      const tool = this.toolById.get(id);
      if (tool) {
        tool.finished = true;
        tool.failed = Boolean(rec.isError);
        const res = rec.result as Record<string, unknown> | undefined;
        if (res && Array.isArray(res.content)) {
          tool.output = (res.content as Record<string, unknown>[])
            .filter((c) => c.type === 'text' && typeof c.text === 'string')
            .map((c) => String(c.text))
            .join('\n')
            .slice(0, 16_000);
        }
        this.bump();
      }
      return;
    }

    if (type === 'extension_ui_request') {
      const method = String(rec.method ?? '');
      if (method === 'setStatus' && rec.statusKey === 'pui-initial-title' && typeof rec.statusText === 'string') {
        try {
          const parsed = JSON.parse(rec.statusText) as { title?: string };
          if (parsed.title && parsed.title.trim()) {
            this.title = parsed.title.trim();
            this.bump();
          }
        } catch {
          // ignore
        }
        return;
      }
      if (method === 'notify' || method === 'setStatus' || method === 'setWidget') {
        return;
      }
      const id = String(rec.id ?? crypto.randomUUID());
      const title = String(rec.title ?? '');
      const placeholder = String(rec.placeholder ?? '');

      // Internal Pui Desktop RPC handshakes from scripts/ultracode.ts:
      // never show these as user approval dialogs or push notifications.
      // When no Desktop /v1/rpc listener is attached, answer immediately so Pi never hangs 30s.
      const isInternalPuiRpc =
        method === 'input' &&
        (title === 'Pui steering window' ||
          title === 'Pui open' ||
          title === 'Pui agent control' ||
          title === 'Pui workflow check');
      if (isInternalPuiRpc) {
        if (this.rpcListeners.size === 0) {
          if (title === 'Pui steering window') {
            this.sendRpc({ type: 'extension_ui_response', id, value: '' });
          } else if (title === 'Pui open') {
            this.sendRpc({
              type: 'extension_ui_response',
              id,
              value: JSON.stringify({ opened: false, note: 'Headless cloud host: file available in workspace.' }),
            });
          } else if (title === 'Pui agent control') {
            let reply: Record<string, unknown> = { ok: false, error: 'Unavailable on headless cloud host.' };
            try {
              const reqObj = JSON.parse(placeholder) as { action?: string };
              if (reqObj.action === 'workflow_tool_guard') {
                reply = { allow: true };
              }
            } catch {
              // ignore
            }
            this.sendRpc({ type: 'extension_ui_response', id, value: JSON.stringify(reply) });
          } else {
            this.sendRpc({ type: 'extension_ui_response', id, cancelled: true });
          }
        }
        return;
      }

      if (!this.dialogs.some((d) => d.id === id)) {
        this.dialogs.push(rec);
        this.bump();
      }
      if (!this.seenDialogIds.has(id)) {
        this.seenDialogIds.add(id);
        const message = String(rec.message ?? '');
        const isQuestion =
          method === 'pui_ask_questions' ||
          title === 'Pui questions' ||
          message.includes('pui.questions.v1') ||
          placeholder.includes('"questions"');
        this.onNotify({
          id: `${isQuestion ? 'question' : 'approval'}:${id}`,
          kind: isQuestion ? 'question' : 'approval',
          path: this.sessionPath,
          title: this.title || 'pui-cloud',
          body: truncateText(title || message || (isQuestion ? 'Agent has a question.' : 'Tool approval requested.')),
          timestamp: Date.now(),
          dialog_id: id,
          dialog_method: method,
        });
      }
    }
  }

  private reloadHistoryFromDisk(): void {
    try {
      if (!fs.existsSync(this.sessionPath)) return;
      const parsed = parseSessionFile(this.sessionPath, true);
      if (parsed.messages.length > 0) {
        this.messages = parsed.messages;
        this.toolById.clear();
        for (const m of this.messages) {
          for (const t of m.tools) this.toolById.set(t.id, t);
        }
      }
    } catch {
      // ignore
    }
  }

  toDetail(): SessionDetail {
    return {
      path: this.sessionPath,
      title: this.title,
      cwd: this.cwd,
      project: this.cwd,
      status: this.dialogs.length > 0 ? 'Needs approval' : this.busy ? 'Working…' : 'Idle',
      busy: this.busy,
      ready: this.ready && !this.busy,
      model: this.model,
      model_name: this.modelName || this.model,
      provider: this.provider,
      thinking: this.thinking,
      levels: this.levels,
      permission_mode: this.permissionMode,
      cost: this.cost,
      context_tokens: this.contextTokens,
      context_window: this.contextWindow,
      error: this.error,
      notice: this.notice,
      origin: this.origin,
      messages: this.messages,
      pending_messages: [],
      waiting_messages: this.queued.map((q, index) => ({
        index,
        text: q.text,
        mode: q.mode,
        image_count: q.images.length,
      })),
      dialogs: this.dialogs,
      commands: this.commands,
      models: this.models,
      artifacts: extractArtifacts(this.messages),
      revision: this.revision,
    };
  }
}
