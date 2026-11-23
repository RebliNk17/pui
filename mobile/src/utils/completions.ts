import type { FileMentionItem, SlashCommandItem } from '../protocol/types';

export interface CompletionToken {
  kind: 'command' | 'file';
  start: number;
  end: number;
  query: string;
}

export const FALLBACK_LOCAL_COMMANDS: SlashCommandItem[] = [
  { name: 'new', description: 'Start a new chat', source: 'pui' },
  { name: 'model', description: 'Choose model and thinking effort', source: 'pui' },
  { name: 'compact', description: 'Compact context; optional instructions', source: 'pui' },
  { name: 'reload', description: 'Reload resources in this chat', source: 'pui' },
  { name: 'stop', description: 'Stop the agent and clear waiting messages', source: 'pui' },
  { name: 'files', description: 'Open workspace file browser', source: 'pui' },
  { name: 'help', description: 'Show composer help', source: 'pui' },
];

/**
 * Mirrors Pui Desktop's `src/app/completions.rs` cursor-aware token parser:
 * - `/` commands trigger only at the start of the composer (`start === 0`)
 * - `@` file mentions trigger on any whitespace-delimited token anywhere in the draft
 */
export function detectCompletionToken(text: string, cursor: number): CompletionToken | null {
  const safeCursor = Math.max(0, Math.min(cursor, text.length));
  const before = text.slice(0, safeCursor);

  let start = before.length;
  while (start > 0 && !/\s/.test(before[start - 1])) {
    start -= 1;
  }
  const word = before.slice(start);
  const isFile = word.startsWith('@');
  const isCommand = start === 0 && word.startsWith('/');
  if (!isFile && !isCommand) {
    return null;
  }

  let end = safeCursor;
  while (end < text.length && !/\s/.test(text[end])) {
    end += 1;
  }

  return {
    kind: isFile ? 'file' : 'command',
    start,
    end,
    query: word.slice(1).toLowerCase(),
  };
}

/**
 * Replaces the active completion token in `text` and returns the new text & cursor position.
 */
export function applyCompletion(
  text: string,
  token: CompletionToken,
  insertValue: string,
): { text: string; cursor: number } {
  const prefix = token.kind === 'command' ? '/' : '@';
  const normalized = insertValue.startsWith(prefix) ? insertValue : `${prefix}${insertValue}`;
  const replacement = `${normalized} `;
  const nextText = `${text.slice(0, token.start)}${replacement}${text.slice(token.end).replace(/^ /, '')}`;
  const nextCursor = token.start + replacement.length;
  return { text: nextText, cursor: nextCursor };
}

export function filterSlashCommands(
  commands: SlashCommandItem[],
  query: string,
): SlashCommandItem[] {
  const needle = query.trim().toLowerCase();
  const merged = new Map<string, SlashCommandItem>();
  for (const cmd of FALLBACK_LOCAL_COMMANDS) {
    merged.set(cmd.name, cmd);
  }
  for (const cmd of commands) {
    merged.set(cmd.name, cmd);
  }
  const all = Array.from(merged.values());
  if (!needle) {
    return all;
  }
  return all
    .filter(
      (cmd) =>
        cmd.name.toLowerCase().includes(needle) ||
        cmd.description.toLowerCase().includes(needle),
    )
    .sort((a, b) => {
      const aStarts = a.name.toLowerCase().startsWith(needle) ? 0 : 1;
      const bStarts = b.name.toLowerCase().startsWith(needle) ? 0 : 1;
      return aStarts - bStarts || a.name.localeCompare(b.name);
    });
}

export function filterFileMentions(files: FileMentionItem[], query: string): FileMentionItem[] {
  const needle = query.trim().toLowerCase();
  if (!needle) {
    return files;
  }
  return files
    .filter((f) => f.path.toLowerCase().includes(needle))
    .sort((a, b) => {
      const aExact = a.name.toLowerCase().startsWith(needle) ? 0 : 1;
      const bExact = b.name.toLowerCase().startsWith(needle) ? 0 : 1;
      return aExact - bExact || a.path.length - b.path.length || a.path.localeCompare(b.path);
    });
}
