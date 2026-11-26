import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import type { SessionSummary } from '../src/protocol/types.ts';
import {
  descendantsOfSession,
  expandSectionsWithSubagents,
  filterSessions,
  formatRelativeAge,
  groupSessions,
} from '../src/utils/sessions.ts';

const NOW = new Date(2026, 10, 23, 20, 0, 0).getTime();

function makeSession(overrides: Partial<SessionSummary>): SessionSummary {
  return {
    path: '/s/a.jsonl',
    title: 'Test chat',
    project: '/repo/pui',
    project_name: 'pui',
    cwd: '/repo/pui',
    status: 'Idle',
    busy: false,
    unread: false,
    has_pending_dialog: false,
    model: 'claude-opus',
    modified_ms: NOW - 10_000,
    created_ms: NOW - 60_000,
    origin: { platform: 'desktop' },
    is_active: false,
    ...overrides,
  };
}

describe('session grouping, ordering, filtering, and relative ages', () => {
  it('formats relative ages compactly', () => {
    assert.equal(formatRelativeAge(NOW - 20_000, NOW), 'now');
    assert.equal(formatRelativeAge(NOW - 14 * 60_000, NOW), '14m');
    assert.equal(formatRelativeAge(NOW - 3 * 3600_000, NOW), '3h');
    assert.equal(formatRelativeAge(NOW - 2 * 86400_000, NOW), '2d');
  });

  it('groups by date buckets, by project, and flat with running-first ordering', () => {
    const items = [
      makeSession({ path: '/1', title: 'Alpha', modified_ms: NOW - 120_000, project_name: 'pui' }),
      makeSession({
        path: '/2',
        title: 'Beta',
        modified_ms: NOW - 26 * 3600_000,
        project_name: 'game',
        busy: true,
        origin: { platform: 'android', device: 'S26 Ultra' },
      }),
      makeSession({
        path: '/3',
        title: 'Gamma',
        modified_ms: NOW - 4 * 86400_000,
        project_name: 'pui',
      }),
    ];

    const byDate = groupSessions(items, 'date', 'recent', NOW);
    assert.deepEqual(
      byDate.map((s) => s.title),
      ['Today', 'Yesterday', 'Last week'],
    );

    const byProject = groupSessions(items, 'project', 'running_first', NOW);
    assert.equal(byProject[0].title, 'game');
    assert.equal(byProject[0].data[0].path, '/2');

    const phoneOnly = filterSessions(items, 'phone', '');
    assert.equal(phoneOnly.length, 1);
    assert.equal(phoneOnly[0].path, '/2');
  });

  it('keeps subagents out of top-level rows until expanded, searched, or needing approval', () => {
    const items = [
      makeSession({ path: '/parent', title: 'Main agent', modified_ms: NOW - 10_000 }),
      makeSession({
        path: '/child-1',
        title: 'local-sandbox',
        parent_session: '/parent',
        modified_ms: NOW - 5_000,
        busy: true,
      }),
      makeSession({
        path: '/child-2',
        title: 'turn-ux',
        parent_session: '/child-1',
        modified_ms: NOW - 4_000,
        has_pending_dialog: true,
      }),
    ];

    // Default browsing hides subagents from the top-level list.
    const allTop = filterSessions(items, 'all', '');
    assert.deepEqual(
      allTop.map((s) => s.path),
      ['/parent'],
    );

    // Running filter keeps parent visible when a subagent is busy.
    const running = filterSessions(items, 'running', '');
    assert.deepEqual(
      running.map((s) => s.path),
      ['/parent'],
    );

    // Attention filter surfaces both parent and subagent with pending dialog.
    const attention = filterSessions(items, 'attention', '');
    assert.deepEqual(
      attention.map((s) => s.path),
      ['/parent', '/child-2'],
    );

    // Explicit search still finds subagents by name.
    const searched = filterSessions(items, 'all', 'local-sandbox');
    assert.deepEqual(
      searched.map((s) => s.path),
      ['/child-1'],
    );

    // Recursive descendants lookup finds nested subagents.
    assert.deepEqual(
      descendantsOfSession(items, '/parent').map((s) => s.path),
      ['/child-2', '/child-1'],
    );

    // Expanding parent splices subagents directly beneath it.
    const grouped = groupSessions(allTop, 'date', 'recent', NOW);
    const expanded = expandSectionsWithSubagents(grouped, items, { '/parent': true });
    assert.deepEqual(
      expanded[0].data.map((s) => s.path),
      ['/parent', '/child-2', '/child-1'],
    );
  });
});
