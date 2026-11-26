import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { daemonHomeDir, piAgentDir } from './config.ts';

export interface SessionOrigin {
  platform: string;
  device?: string | null;
  client?: string | null;
  created_at?: number | null;
}

export interface ChatMeta {
  title?: string;
  pinned?: boolean;
  hidden?: boolean;
  origin?: SessionOrigin;
  parent_session?: string;
  branch?: string;
  base_branch?: string;
  repo_dir?: string;
}

export interface ToolRecord {
  id: string;
  name: string;
  args: string;
  output: string;
  failed: boolean;
  finished: boolean;
  path: string;
  command: string;
  diff: string;
  added: number;
  removed: number;
  code: string;
  children: ToolRecord[];
  images: { data: string; mime: string; file?: { name?: string } }[];
  visual?: { title: string; html: string; height?: number };
  open_target?: { target: string; kind: string; title?: string };
}

export interface MessageRecord {
  role: string;
  text: string;
  thinking: string;
  provider: string;
  model: string;
  tools: ToolRecord[];
  images: { data: string; mime: string; file?: { name?: string } }[];
  timestamp: number;
  cost: number;
  effort: string;
  error?: string;
  revision: number;
}

export interface ArtifactItem {
  id: string;
  kind: string;
  title: string;
  html?: string;
  height?: number;
  path?: string;
  mime?: string;
  data_base64?: string;
  message_index: number;
}

export interface SessionSummary {
  path: string;
  title: string;
  project: string;
  project_name: string;
  cwd: string;
  branch?: string | null;
  status: string;
  busy: boolean;
  unread: boolean;
  has_pending_dialog: boolean;
  model: string;
  modified_ms: number;
  created_ms?: number | null;
  origin: SessionOrigin;
  parent_session?: string | null;
  pinned: boolean;
  is_active: boolean;
}

export function defaultCloudOrigin(): SessionOrigin {
  return { platform: 'desktop', device: 'pui-cloud', client: 'pui-daemon' };
}

function metaFilePath(): string {
  return path.join(daemonHomeDir(), 'meta.json');
}

export function loadAllMeta(): Record<string, ChatMeta> {
  try {
    const raw = fs.readFileSync(metaFilePath(), 'utf8');
    const parsed = JSON.parse(raw) as unknown;
    return parsed && typeof parsed === 'object' ? (parsed as Record<string, ChatMeta>) : {};
  } catch {
    return {};
  }
}

export function saveAllMeta(meta: Record<string, ChatMeta>): void {
  fs.mkdirSync(daemonHomeDir(), { recursive: true });
  fs.writeFileSync(metaFilePath(), JSON.stringify(meta, null, 2) + '\n', { mode: 0o600 });
}

export function sessionsRootDir(): string {
  return path.join(piAgentDir(), 'sessions');
}

export function encodeSessionDirName(cwd: string): string {
  const cleaned = cwd.replace(/^\/+/, '').replace(/[^a-zA-Z0-9._-]+/g, '-');
  return `--${cleaned || 'root'}--`;
}

export function createEmptySessionFile(cwd: string, origin?: SessionOrigin): string {
  const dir = path.join(sessionsRootDir(), encodeSessionDirName(cwd));
  fs.mkdirSync(dir, { recursive: true });
  const id = crypto.randomUUID();
  const ts = new Date().toISOString().replace(/[:.]/g, '-');
  const filePath = path.join(dir, `${ts}_${id}.jsonl`);
  const header = {
    type: 'session',
    version: 3,
    id,
    timestamp: new Date().toISOString(),
    cwd,
  };
  const lines = [JSON.stringify(header)];
  if (origin) {
    lines.push(
      JSON.stringify({
        type: 'custom',
        customType: 'pui.session-origin.v1',
        data: origin,
        timestamp: new Date().toISOString(),
      }),
    );
  }
  fs.writeFileSync(filePath, lines.join('\n') + '\n', 'utf8');
  return filePath;
}

export function forkSessionFile(sourcePath: string, origin?: SessionOrigin): string {
  const raw = fs.readFileSync(sourcePath, 'utf8');
  const dir = path.dirname(sourcePath);
  const newId = crypto.randomUUID();
  const ts = new Date().toISOString().replace(/[:.]/g, '-');
  const target = path.join(dir, `${ts}_${newId}.jsonl`);
  const lines = raw.split('\n').filter((l) => l.length > 0);
  if (lines.length > 0) {
    try {
      const hdr = JSON.parse(lines[0]) as Record<string, unknown>;
      if (hdr.type === 'session') {
        hdr.id = newId;
        hdr.timestamp = new Date().toISOString();
        hdr.parentSession = sourcePath;
        lines[0] = JSON.stringify(hdr);
      }
    } catch {
      // keep first line
    }
  }
  if (origin) {
    lines.push(
      JSON.stringify({
        type: 'custom',
        customType: 'pui.session-origin.v1',
        data: origin,
        timestamp: new Date().toISOString(),
      }),
    );
  }
  fs.writeFileSync(target, lines.join('\n') + '\n', 'utf8');
  return target;
}

export function extractArtifacts(messages: MessageRecord[]): ArtifactItem[] {
  const out: ArtifactItem[] = [];
  const seenPaths = new Set<string>();
  let seq = 0;
  messages.forEach((msg, mIdx) => {
    if (msg.text) {
      const mdImgRe = /!\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;
      let m: RegExpExecArray | null;
      while ((m = mdImgRe.exec(msg.text)) !== null) {
        const alt = (m[1] || '').trim();
        const rawTarget = (m[2] || '').trim().replace(/^<|>$/g, '');
        if (
          rawTarget &&
          !/^(https?:\/\/|data:)/i.test(rawTarget) &&
          /\.(png|jpe?g|gif|webp|bmp|svg)$/i.test(rawTarget) &&
          !seenPaths.has(rawTarget)
        ) {
          seenPaths.add(rawTarget);
          out.push({
            id: `art-${seq++}`,
            kind: 'image',
            title: alt || path.basename(rawTarget) || rawTarget,
            path: rawTarget,
            message_index: mIdx,
          });
        }
      }
    }
    for (const tool of msg.tools) {
      if (tool.visual) {
        out.push({
          id: `art-${seq++}`,
          kind: 'html',
          title: tool.visual.title || 'Visual',
          html: tool.visual.html,
          height: tool.visual.height,
          message_index: mIdx,
        });
      }
      if (tool.open_target) {
        out.push({
          id: `art-${seq++}`,
          kind: tool.open_target.kind || 'file',
          title: tool.open_target.title || tool.open_target.target,
          path: tool.open_target.target,
          message_index: mIdx,
        });
      }
      for (const img of tool.images) {
        out.push({
          id: `art-${seq++}`,
          kind: 'image',
          title: img.file?.name || `${tool.name} image`,
          mime: img.mime || 'image/png',
          data_base64: img.data,
          message_index: mIdx,
        });
      }
    }
  });
  return out;
}

export function parsePiContentToMessage(
  rawMsg: Record<string, unknown>,
  revision = 1,
): MessageRecord {
  const role = typeof rawMsg.role === 'string' ? rawMsg.role : 'assistant';
  const content = rawMsg.content;
  const textParts: string[] = [];
  const thinkingParts: string[] = [];
  const tools: ToolRecord[] = [];
  const images: { data: string; mime: string }[] = [];

  if (typeof content === 'string') {
    textParts.push(content);
  } else if (Array.isArray(content)) {
    for (const block of content) {
      if (!block || typeof block !== 'object') continue;
      const b = block as Record<string, unknown>;
      if (b.type === 'text' && typeof b.text === 'string') {
        textParts.push(b.text);
      } else if (b.type === 'thinking' && typeof b.thinking === 'string') {
        thinkingParts.push(b.thinking);
      } else if (b.type === 'image' && typeof b.data === 'string') {
        images.push({
          data: b.data,
          mime: typeof b.mimeType === 'string' ? b.mimeType : 'image/png',
        });
      } else if (b.type === 'toolCall') {
        const name = typeof b.name === 'string' ? b.name : 'tool';
        const argsObj =
          b.arguments && typeof b.arguments === 'object'
            ? (b.arguments as Record<string, unknown>)
            : {};
        const tool: ToolRecord = {
          id: typeof b.id === 'string' ? b.id : crypto.randomUUID(),
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
        if (name === 'pui_visual' && typeof argsObj.html === 'string') {
          tool.visual = {
            title: typeof argsObj.title === 'string' ? argsObj.title : 'Visual',
            html: argsObj.html,
            height: typeof argsObj.height === 'number' ? argsObj.height : undefined,
          };
        }
        if (name === 'pui_open' && typeof argsObj.target === 'string') {
          tool.open_target = {
            target: argsObj.target,
            kind: /^https?:\/\//i.test(argsObj.target) ? 'url' : 'file',
          };
        }
        tools.push(tool);
      }
    }
  }

  const usage =
    rawMsg.usage && typeof rawMsg.usage === 'object'
      ? (rawMsg.usage as Record<string, unknown>)
      : null;
  const costObj =
    usage?.cost && typeof usage.cost === 'object'
      ? (usage.cost as Record<string, unknown>)
      : null;

  return {
    role,
    text: textParts.join('\n'),
    thinking: thinkingParts.join('\n'),
    provider: typeof rawMsg.provider === 'string' ? rawMsg.provider : '',
    model: typeof rawMsg.model === 'string' ? rawMsg.model : '',
    tools,
    images,
    timestamp: typeof rawMsg.timestamp === 'number' ? rawMsg.timestamp : Date.now(),
    cost: typeof costObj?.total === 'number' ? costObj.total : 0,
    effort: '',
    error: typeof rawMsg.errorMessage === 'string' ? rawMsg.errorMessage : undefined,
    revision,
  };
}

export interface ParsedSessionFile {
  path: string;
  cwd: string;
  title: string;
  created_ms: number | null;
  modified_ms: number;
  model: string;
  origin: SessionOrigin;
  parent_session: string | null;
  messages: MessageRecord[];
}

export function parseSessionFile(filePath: string, includeMessages = true): ParsedSessionFile {
  const stat = fs.statSync(filePath);
  const raw = fs.readFileSync(filePath, 'utf8');
  let cwd = '';
  let explicitName = '';
  let firstUserText = '';
  let created_ms: number | null = null;
  let model = '';
  let origin: SessionOrigin = defaultCloudOrigin();
  let parent_session: string | null = null;

  const messages: MessageRecord[] = [];
  const toolById = new Map<string, ToolRecord>();

  for (const line of raw.split('\n')) {
    if (!line.trim()) continue;
    let entry: Record<string, unknown>;
    try {
      entry = JSON.parse(line) as Record<string, unknown>;
    } catch {
      continue;
    }

    if (entry.type === 'session') {
      if (typeof entry.cwd === 'string') cwd = entry.cwd;
      if (typeof entry.parentSession === 'string') parent_session = entry.parentSession;
      if (typeof entry.timestamp === 'string') {
        const parsedTs = Date.parse(entry.timestamp);
        if (Number.isFinite(parsedTs)) created_ms = parsedTs;
      }
    } else if (entry.type === 'session_info' && typeof entry.name === 'string') {
      explicitName = entry.name.trim();
    } else if (entry.type === 'model_change' && typeof entry.modelId === 'string') {
      model = entry.modelId;
    } else if (entry.type === 'custom' && entry.customType === 'pui.session-origin.v1') {
      if (entry.data && typeof entry.data === 'object') {
        origin = entry.data as SessionOrigin;
      }
    } else if (entry.type === 'message' && entry.message && typeof entry.message === 'object') {
      const m = entry.message as Record<string, unknown>;
      if (m.role === 'toolResult') {
        if (includeMessages && typeof m.toolCallId === 'string') {
          const tool = toolById.get(m.toolCallId);
          if (tool) {
            tool.finished = true;
            tool.failed = Boolean(m.isError);
            const parts: string[] = [];
            if (Array.isArray(m.content)) {
              for (const c of m.content) {
                if (c && typeof c === 'object') {
                  const block = c as Record<string, unknown>;
                  if (block.type === 'text' && typeof block.text === 'string') parts.push(block.text);
                  if (block.type === 'image' && typeof block.data === 'string') {
                    tool.images.push({
                      data: block.data,
                      mime: typeof block.mimeType === 'string' ? block.mimeType : 'image/png',
                    });
                  }
                }
              }
            }
            tool.output = parts.join('\n').slice(0, 16_000);
          }
        }
        continue;
      }

      if (m.role === 'user' || m.role === 'assistant') {
        if (typeof m.model === 'string' && m.model) model = m.model;
        if (includeMessages || (!firstUserText && m.role === 'user')) {
          const parsed = parsePiContentToMessage(m, messages.length + 1);
          if (m.role === 'user' && !firstUserText && parsed.text.trim()) {
            firstUserText = parsed.text.trim().split('\n')[0].slice(0, 90);
          }
          if (includeMessages) {
            for (const t of parsed.tools) {
              toolById.set(t.id, t);
            }
            messages.push(parsed);
          }
        }
      }
    }
  }

  const title = explicitName || firstUserText || 'New conversation';
  return {
    path: filePath,
    cwd,
    title,
    created_ms,
    modified_ms: Math.floor(stat.mtimeMs),
    model,
    origin,
    parent_session,
    messages,
  };
}

export function scanSavedSessions(meta: Record<string, ChatMeta>): ParsedSessionFile[] {
  const root = sessionsRootDir();
  if (!fs.existsSync(root)) return [];
  const results: ParsedSessionFile[] = [];

  const walk = (dir: string, depth: number) => {
    if (depth > 3) return;
    let ents: fs.Dirent[];
    try {
      ents = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const ent of ents) {
      const full = path.join(dir, ent.name);
      if (ent.isDirectory()) {
        walk(full, depth + 1);
      } else if (ent.isFile() && ent.name.endsWith('.jsonl')) {
        if (meta[full]?.hidden) continue;
        try {
          results.push(parseSessionFile(full, false));
        } catch {
          // skip unreadable file
        }
      }
    }
  };
  walk(root, 0);
  results.sort((a, b) => b.modified_ms - a.modified_ms);
  return results.slice(0, 200);
}
