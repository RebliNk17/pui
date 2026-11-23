import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import type { SessionSummary } from '../src/protocol/types.ts';
import {
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
});
