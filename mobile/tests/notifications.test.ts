import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import type { NotificationEvent } from '../src/protocol/types.ts';
import {
  formatNotificationContent,
  shouldPresentNotification,
  type NotificationPrefs,
} from '../src/utils/notifications.ts';

const ALL_ON: NotificationPrefs = {
  notificationsEnabled: true,
  notifyFinished: true,
  notifyApprovals: true,
  notifyQuestions: true,
};

function ev(kind: NotificationEvent['kind'], path = '/tmp/a.jsonl'): NotificationEvent {
  return {
    id: `${kind}-1`,
    kind,
    path,
    title: 'Refactor auth',
    body: 'Done refactoring.',
    timestamp: 1000,
  };
}

describe('mobile notification filtering and formatting', () => {
  it('suppresses alerts when user is actively viewing the same chat in foreground', () => {
    assert.equal(
      shouldPresentNotification(ev('finished'), ALL_ON, {
        appState: 'active',
        activeChatPath: '/tmp/a.jsonl',
      }),
      false,
    );
    // Different chat in foreground still notifies.
    assert.equal(
      shouldPresentNotification(ev('finished'), ALL_ON, {
        appState: 'active',
        activeChatPath: '/tmp/b.jsonl',
      }),
      true,
    );
    // Same chat while app is backgrounded notifies.
    assert.equal(
      shouldPresentNotification(ev('finished'), ALL_ON, {
        appState: 'background',
        activeChatPath: '/tmp/a.jsonl',
      }),
      true,
    );
  });

  it('respects master toggle, per-kind toggles, and deduplication set', () => {
    const seen = new Set<string>(['approval-1']);
    assert.equal(
      shouldPresentNotification(ev('approval'), ALL_ON, { appState: 'background', activeChatPath: null }, seen),
      false,
    );
    assert.equal(
      shouldPresentNotification(
        ev('question'),
        { ...ALL_ON, notifyQuestions: false },
        { appState: 'background', activeChatPath: null },
      ),
      false,
    );
    assert.equal(
      shouldPresentNotification(
        ev('finished'),
        { ...ALL_ON, notificationsEnabled: false },
        { appState: 'background', activeChatPath: null },
      ),
      false,
    );
  });

  it('routes approvals and questions to the high-priority attention channel', () => {
    const fin = formatNotificationContent(ev('finished'));
    assert.equal(fin.channelId, 'pui-agent-finished');
    assert.equal(fin.title, 'Refactor auth');

    const app = formatNotificationContent(ev('approval'));
    assert.equal(app.channelId, 'pui-agent-attention');
    assert.ok(app.title.startsWith('Approval needed'));

    const q = formatNotificationContent(ev('question'));
    assert.equal(q.channelId, 'pui-agent-attention');
    assert.ok(q.title.startsWith('Question'));
  });
});
