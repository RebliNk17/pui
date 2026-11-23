import type { MessageRecord, ToolRecord } from '../protocol/types';

export type TranscriptEntry =
  | {
      kind: 'user';
      key: string;
      message: MessageRecord;
    }
  | {
      kind: 'activity';
      key: string;
      tools: ToolRecord[];
      thinking: string[];
      summary: string;
      hasRunning: boolean;
      hasError: boolean;
    }
  | {
      kind: 'assistant';
      key: string;
      message: MessageRecord;
      /** Only visual/open/image tools attached directly to this reply. */
      inlineTools: ToolRecord[];
    };

function plural(count: number, singular: string, pluralWord: string): string {
  return count === 1 ? `1 ${singular}` : `${count} ${pluralWord}`;
}

/**
 * Formats a grouped list of tools into Pui Desktop's concise human-readable line,
 * e.g. "read 4 files · ran 3 commands · edited 2 files".
 */
export function summarizeActivity(tools: ToolRecord[], thinkingCount = 0): string {
  let reads = 0;
  let edits = 0;
  let commands = 0;
  let searches = 0;
  const otherCounts = new Map<string, number>();

  for (const t of tools) {
    const name = t.name.toLowerCase();
    if (name === 'read') reads += 1;
    else if (name === 'edit' || name === 'write') edits += 1;
    else if (name === 'bash' || name === 'shell') commands += 1;
    else if (name === 'grep' || name === 'find' || name === 'ls' || name === 'glob') searches += 1;
    else if (name === 'pui_ask_questions' || name === 'ask_questions') {
      otherCounts.set('asked questions', (otherCounts.get('asked questions') ?? 0) + 1);
    } else if (name === 'pui_open') {
      otherCounts.set('opened file', (otherCounts.get('opened file') ?? 0) + 1);
    } else if (name === 'pui_visual') {
      continue; // visuals render inline directly
    } else {
      otherCounts.set(t.name, (otherCounts.get(t.name) ?? 0) + 1);
    }
  }

  const parts: string[] = [];
  if (reads > 0) parts.push(`read ${plural(reads, 'file', 'files')}`);
  if (searches > 0) parts.push(`searched ${plural(searches, 'time', 'times')}`);
  if (commands > 0) parts.push(`ran ${plural(commands, 'command', 'commands')}`);
  if (edits > 0) parts.push(`edited ${plural(edits, 'file', 'files')}`);
  for (const [label, count] of otherCounts.entries()) {
    parts.push(count > 1 ? `${label} ×${count}` : label);
  }
  if (parts.length === 0 && thinkingCount > 0) {
    parts.push('thought');
  }
  return parts.join(' · ') || 'activity';
}

/**
 * Collapses consecutive tool-calling assistant turns into ONE grouped activity row
 * (just like Pui Desktop), while keeping inline visuals (`pui_visual`), `pui_open` chips,
 * images, and assistant prose visible in order.
 */
export function groupTranscript(messages: MessageRecord[]): TranscriptEntry[] {
  const entries: TranscriptEntry[] = [];
  let pendingTools: ToolRecord[] = [];
  let pendingThinking: string[] = [];
  let activityIndex = 0;

  const flushActivity = () => {
    if (pendingTools.length === 0 && pendingThinking.length === 0) return;
    const summary = summarizeActivity(pendingTools, pendingThinking.length);
    entries.push({
      kind: 'activity',
      key: `activity-${activityIndex}`,
      tools: pendingTools,
      thinking: pendingThinking,
      summary,
      hasRunning: pendingTools.some((t) => !t.finished && !t.failed),
      hasError: pendingTools.some((t) => t.failed),
    });
    activityIndex += 1;
    pendingTools = [];
    pendingThinking = [];
  };

  for (let i = 0; i < messages.length; i += 1) {
    const msg = messages[i];
    if (msg.role === 'user') {
      flushActivity();
      entries.push({
        kind: 'user',
        key: `user-${msg.timestamp}-${i}`,
        message: msg,
      });
      continue;
    }

    // Assistant message: collect reasoning + non-visual tool steps into the running group.
    if (msg.thinking && msg.thinking.trim()) {
      pendingThinking.push(msg.thinking.trim());
    }
    const inlineTools: ToolRecord[] = [];
    for (const tool of msg.tools) {
      if (tool.visual || tool.open_target) {
        inlineTools.push(tool);
      }
      if (!tool.visual) {
        pendingTools.push(tool);
      }
    }

    const hasVisibleReply =
      Boolean(msg.text.trim()) ||
      Boolean(msg.error) ||
      msg.images.length > 0 ||
      inlineTools.length > 0;

    if (hasVisibleReply) {
      flushActivity();
      entries.push({
        kind: 'assistant',
        key: `asst-${msg.timestamp}-${i}`,
        message: msg,
        inlineTools,
      });
    }
  }

  flushActivity();
  return entries;
}
