import type { NotificationEvent } from '../protocol/types';

export interface NotificationPrefs {
  notificationsEnabled: boolean;
  notifyFinished: boolean;
  notifyApprovals: boolean;
  notifyQuestions: boolean;
}

export interface NotificationContext {
  appState: 'active' | 'background' | 'inactive' | string;
  activeChatPath: string | null;
}

export interface FormattedNotification {
  title: string;
  body: string;
  channelId: 'pui-agent-finished' | 'pui-agent-attention';
}

export function shouldPresentNotification(
  event: NotificationEvent,
  prefs: NotificationPrefs,
  ctx: NotificationContext,
  seenIds?: Set<string>,
): boolean {
  if (!prefs.notificationsEnabled) return false;
  if (seenIds?.has(event.id)) return false;

  if (event.kind === 'finished' && !prefs.notifyFinished) return false;
  if (event.kind === 'approval' && !prefs.notifyApprovals) return false;
  if (event.kind === 'question' && !prefs.notifyQuestions) return false;

  // Suppress alerts while the user is actively looking at this exact chat in foreground.
  if (ctx.appState === 'active' && ctx.activeChatPath && ctx.activeChatPath === event.path) {
    return false;
  }

  return true;
}

export function formatNotificationContent(event: NotificationEvent): FormattedNotification {
  const sessionTitle = event.title.trim() || 'Pui';
  if (event.kind === 'approval') {
    return {
      title: `Approval needed · ${sessionTitle}`,
      body: event.body.trim() || 'A tool call is waiting for your approval.',
      channelId: 'pui-agent-attention',
    };
  }
  if (event.kind === 'question') {
    return {
      title: `Question · ${sessionTitle}`,
      body: event.body.trim() || 'Pi is waiting for your answer.',
      channelId: 'pui-agent-attention',
    };
  }
  return {
    title: sessionTitle,
    body: event.body.trim() || 'Pi finished replying.',
    channelId: 'pui-agent-finished',
  };
}
