// Pure helpers for bounded per-chat draft storage keys & maps.

export const DRAFTS_STORAGE_KEY = 'pui_mobile_chat_drafts_v1';
export const MAX_SAVED_DRAFTS = 30;
export const MAX_DRAFT_CHARS = 12_000;

export type DraftMap = Record<string, { text: string; updatedAt: number }>;

export function parseDraftMap(raw: string | null | undefined): DraftMap {
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
    const out: DraftMap = {};
    for (const [key, val] of Object.entries(parsed as Record<string, unknown>)) {
      if (
        val &&
        typeof val === 'object' &&
        typeof (val as { text?: unknown }).text === 'string' &&
        typeof (val as { updatedAt?: unknown }).updatedAt === 'number'
      ) {
        out[key] = {
          text: (val as { text: string }).text,
          updatedAt: (val as { updatedAt: number }).updatedAt,
        };
      }
    }
    return out;
  } catch {
    return {};
  }
}

export function upsertDraft(
  map: DraftMap,
  sessionPath: string,
  text: string,
  now = Date.now(),
): DraftMap {
  const next: DraftMap = { ...map };
  if (!sessionPath) return next;
  if (!text.trim()) {
    delete next[sessionPath];
    return next;
  }
  next[sessionPath] = {
    text: text.slice(0, MAX_DRAFT_CHARS),
    updatedAt: now,
  };
  const keys = Object.keys(next);
  if (keys.length > MAX_SAVED_DRAFTS) {
    keys.sort((a, b) => next[b].updatedAt - next[a].updatedAt);
    for (const stale of keys.slice(MAX_SAVED_DRAFTS)) {
      delete next[stale];
    }
  }
  return next;
}
