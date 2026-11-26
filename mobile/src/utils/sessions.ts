import type { SessionSummary } from '../protocol/types';

export type SessionGrouping = 'date' | 'project' | 'none';
export type SessionOrdering = 'recent' | 'running_first' | 'created' | 'title';
export type SessionFilter = 'all' | 'phone' | 'running' | 'attention';

export interface SessionSection {
  key: string;
  title: string;
  count: number;
  data: SessionSummary[];
}

export function formatRelativeAge(timestampMs: number, nowMs = Date.now()): string {
  if (!timestampMs || timestampMs <= 0) return '';
  const diffSec = Math.max(0, Math.floor((nowMs - timestampMs) / 1000));
  if (diffSec < 60) return 'now';
  const mins = Math.floor(diffSec / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d`;
  const months = Math.floor(days / 30);
  return `${months}mo`;
}

export function dateBucketFor(timestampMs: number, nowMs = Date.now()): string {
  if (!timestampMs || timestampMs <= 0) return 'Older';
  const now = new Date(nowMs);
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const startOfYesterday = startOfToday - 24 * 60 * 60 * 1000;
  const startOfLastWeek = startOfToday - 7 * 24 * 60 * 60 * 1000;
  if (timestampMs >= startOfToday) return 'Today';
  if (timestampMs >= startOfYesterday) return 'Yesterday';
  if (timestampMs >= startOfLastWeek) return 'Last week';
  return 'Older';
}

export function filterSessions(
  sessions: SessionSummary[],
  filter: SessionFilter,
  query: string,
): SessionSummary[] {
  const q = query.trim().toLowerCase();
  return sessions.filter((s) => {
    const isSubagent = Boolean(s.parent_session);
    // Keep subagents out of the top-level session list during normal browsing;
    // they live under their parent (expandable in the list and in the chat's
    // Subagents bar). Still surface subagents when searching or when one needs
    // approval in the Alerts filter.
    if (!q && isSubagent) {
      if (filter === 'attention' && s.has_pending_dialog) {
        return true;
      }
      return false;
    }

    const descendants = isSubagent ? [] : descendantsOfSession(sessions, s.path);
    if (filter === 'phone' && s.origin.platform !== 'android' && s.origin.platform !== 'ios') {
      return false;
    }
    if (filter === 'running' && !s.busy && !descendants.some((d) => d.busy)) {
      return false;
    }
    if (
      filter === 'attention' &&
      !s.has_pending_dialog &&
      !s.unread &&
      !descendants.some((d) => d.has_pending_dialog || d.unread)
    ) {
      return false;
    }
    if (!q) return true;
    return (
      s.title.toLowerCase().includes(q) ||
      s.project_name.toLowerCase().includes(q) ||
      s.cwd.toLowerCase().includes(q) ||
      (s.origin.device ?? '').toLowerCase().includes(q)
    );
  });
}

export function sortSessions(
  sessions: SessionSummary[],
  ordering: SessionOrdering,
): SessionSummary[] {
  const copy = [...sessions];
  copy.sort((a, b) => {
    if (ordering === 'running_first') {
      const aPri = a.has_pending_dialog ? 2 : a.busy ? 1 : 0;
      const bPri = b.has_pending_dialog ? 2 : b.busy ? 1 : 0;
      if (aPri !== bPri) return bPri - aPri;
      return b.modified_ms - a.modified_ms;
    }
    if (ordering === 'created') {
      const aCreated = a.created_ms ?? a.modified_ms;
      const bCreated = b.created_ms ?? b.modified_ms;
      return bCreated - aCreated;
    }
    if (ordering === 'title') {
      return a.title.localeCompare(b.title, undefined, { sensitivity: 'base' });
    }
    // 'recent'
    return b.modified_ms - a.modified_ms;
  });
  return copy;
}

export function childrenOfSession(
  sessions: SessionSummary[],
  parentPath: string,
): SessionSummary[] {
  return sessions
    .filter((s) => s.parent_session === parentPath)
    .sort((a, b) => b.modified_ms - a.modified_ms);
}

/** Recursively collect direct and nested subagent sessions for `parentPath`. */
export function descendantsOfSession(
  sessions: SessionSummary[],
  parentPath: string,
): SessionSummary[] {
  const out: SessionSummary[] = [];
  const visited = new Set<string>([parentPath]);
  const queue = [parentPath];
  while (queue.length > 0) {
    const current = queue.shift()!;
    for (const s of sessions) {
      if (s.parent_session === current && !visited.has(s.path)) {
        visited.add(s.path);
        out.push(s);
        queue.push(s.path);
      }
    }
  }
  out.sort((a, b) => b.modified_ms - a.modified_ms);
  return out;
}

/**
 * Splice expanded subagent sessions directly below their parent row in each
 * grouped section without duplicating any already-present row.
 */
export function expandSectionsWithSubagents(
  sections: SessionSection[],
  allSessions: SessionSummary[],
  expandedParents: Record<string, boolean>,
): SessionSection[] {
  return sections.map((section) => {
    const nextData: SessionSummary[] = [];
    const seen = new Set<string>(section.data.map((s) => s.path));
    for (const item of section.data) {
      nextData.push(item);
      if (!item.parent_session && expandedParents[item.path]) {
        for (const child of descendantsOfSession(allSessions, item.path)) {
          if (!seen.has(child.path)) {
            seen.add(child.path);
            nextData.push(child);
          }
        }
      }
    }
    return { ...section, data: nextData };
  });
}

export function groupSessions(
  sessions: SessionSummary[],
  grouping: SessionGrouping,
  ordering: SessionOrdering,
  nowMs = Date.now(),
): SessionSection[] {
  const pinned = sortSessions(
    sessions.filter((s) => Boolean(s.pinned)),
    ordering,
  );
  const rest = sortSessions(
    sessions.filter((s) => !s.pinned),
    ordering,
  );
  const sections: SessionSection[] = [];
  if (pinned.length > 0) {
    sections.push({
      key: 'pinned',
      title: 'Pinned',
      count: pinned.length,
      data: pinned,
    });
  }
  const sorted = rest;
  if (grouping === 'none') {
    if (sorted.length > 0) {
      sections.push({ key: 'all', title: '', count: sorted.length, data: sorted });
    }
    return sections;
  }
  if (grouping === 'project') {
    const map = new Map<string, SessionSummary[]>();
    for (const s of sorted) {
      const key = s.project_name || 'Workspace';
      const bucket = map.get(key) ?? [];
      bucket.push(s);
      map.set(key, bucket);
    }
    for (const [title, data] of map.entries()) {
      sections.push({
        key: `project:${title}`,
        title,
        count: data.length,
        data,
      });
    }
    return sections;
  }

  // 'date' grouping (Today / Yesterday / Last week / Older — matches Pui Desktop sidebar).
  const order = ['Today', 'Yesterday', 'Last week', 'Older'];
  const map = new Map<string, SessionSummary[]>();
  for (const s of sorted) {
    const ts = ordering === 'created' ? (s.created_ms ?? s.modified_ms) : s.modified_ms;
    const bucket = dateBucketFor(ts, nowMs);
    const list = map.get(bucket) ?? [];
    list.push(s);
    map.set(bucket, list);
  }
  for (const label of order) {
    const data = map.get(label);
    if (data && data.length > 0) {
      sections.push({ key: `date:${label}`, title: label, count: data.length, data });
    }
  }
  return sections;
}
