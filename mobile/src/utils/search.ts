import type { TranscriptEntry } from './transcript';

/**
 * Returns indices into `entries` (matching the FlatList `data` array order) whose
 * visible text, error, or tool summary contains `query` (case-insensitive).
 */
export function findMatchingEntryIndices(entries: TranscriptEntry[], query: string): number[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const hits: number[] = [];
  for (let i = 0; i < entries.length; i += 1) {
    const e = entries[i];
    if (e.kind === 'activity') {
      const hay = [
        e.summary,
        ...e.thinking,
        ...e.tools.map((t) => `${t.name} ${t.path} ${t.command} ${t.output}`),
      ]
        .join('\n')
        .toLowerCase();
      if (hay.includes(q)) hits.push(i);
    } else {
      const hay = `${e.message.text}\n${e.message.error ?? ''}`.toLowerCase();
      if (hay.includes(q)) hits.push(i);
    }
  }
  return hits;
}
