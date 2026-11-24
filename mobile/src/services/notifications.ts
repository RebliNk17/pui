import { Platform } from 'react-native';
import * as Notifications from 'expo-notifications';
import type { NotificationEvent } from '../protocol/types';
import { formatNotificationContent } from '../utils/notifications';

export const APPROVAL_CATEGORY_ID = 'pui-approval';
export const ACTION_APPROVE = 'pui-approve';
export const ACTION_DENY = 'pui-deny';

let initialized = false;

export async function initNotifications(): Promise<void> {
  if (initialized || Platform.OS === 'web') return;
  initialized = true;

  Notifications.setNotificationHandler({
    handleNotification: async () => ({
      shouldShowBanner: true,
      shouldShowList: true,
      shouldPlaySound: true,
      shouldSetBadge: false,
    }),
  });

  // Register interactive Approve / Deny lock-screen buttons for tool approval notifications.
  await Notifications.setNotificationCategoryAsync(APPROVAL_CATEGORY_ID, [
    {
      identifier: ACTION_APPROVE,
      buttonTitle: 'Approve',
      options: { opensAppToForeground: false },
    },
    {
      identifier: ACTION_DENY,
      buttonTitle: 'Deny',
      options: { opensAppToForeground: false, isDestructive: true },
    },
  ]).catch(() => {});

  if (Platform.OS === 'android') {
    const ensureChannel = async (
      id: string,
      name: string,
      description: string,
      importance: Notifications.AndroidImportance,
      vibrationPattern: number[],
      lightColor: string,
    ) => {
      try {
        await Notifications.setNotificationChannelAsync(id, {
          name,
          description,
          importance,
          vibrationPattern,
          lightColor,
          enableVibrate: true,
        });
      } catch {
        // Fallback for Expo Go / Android runtimes that reject optional channel fields.
        try {
          await Notifications.setNotificationChannelAsync(id, {
            name,
            importance,
          });
        } catch {
          // Ignore if running in an environment without channel manager support.
        }
      }
    };

    await ensureChannel(
      'pui-agent-finished',
      'Agent Replies',
      'Notifies when Pi finishes replying to a conversation.',
      Notifications.AndroidImportance.HIGH,
      [0, 180, 90, 180],
      '#8B7AE8',
    );
    await ensureChannel(
      'pui-agent-attention',
      'Approvals & Questions',
      'Urgent alerts when Pi needs tool approval or asks a question.',
      Notifications.AndroidImportance.MAX,
      [0, 250, 120, 250],
      '#E5A93B',
    );
  }
}

export async function getNotificationPermissionStatus(): Promise<
  'granted' | 'denied' | 'undetermined'
> {
  if (Platform.OS === 'web') return 'denied';
  try {
    const res = await Notifications.getPermissionsAsync();
    if (res.granted) return 'granted';
    return res.canAskAgain ? 'undetermined' : 'denied';
  } catch {
    return 'undetermined';
  }
}

export async function requestNotificationPermission(): Promise<
  'granted' | 'denied' | 'undetermined'
> {
  if (Platform.OS === 'web') return 'denied';
  try {
    await initNotifications();
    const res = await Notifications.requestPermissionsAsync();
    if (res.granted) return 'granted';
    return res.canAskAgain ? 'undetermined' : 'denied';
  } catch {
    return 'undetermined';
  }
}

export async function presentAgentNotification(event: NotificationEvent): Promise<void> {
  if (Platform.OS === 'web') return;
  try {
    await initNotifications();
    const status = await getNotificationPermissionStatus();
    if (status !== 'granted') return;

    const formatted = formatNotificationContent(event);
    const canQuickApprove = event.kind === 'approval' && Boolean(event.dialog_id);

    await Notifications.scheduleNotificationAsync({
      content: {
        title: formatted.title,
        body: formatted.body,
        sound: 'default',
        ...(canQuickApprove ? { categoryIdentifier: APPROVAL_CATEGORY_ID } : {}),
        data: {
          path: event.path,
          kind: event.kind,
          dialogId: event.dialog_id ?? null,
          dialogMethod: event.dialog_method ?? null,
        },
        ...(Platform.OS === 'android' ? { channelId: formatted.channelId } : {}),
      },
      trigger: null,
    });
  } catch {
    // Ignore native notification failures (e.g. Expo Go restrictions).
  }
}

export async function sendTestNotification(): Promise<boolean> {
  if (Platform.OS === 'web') return false;
  try {
    const status = await requestNotificationPermission();
    if (status !== 'granted') return false;

    await Notifications.scheduleNotificationAsync({
      content: {
        title: 'Pui Notifications Active',
        body: 'You will be alerted when Pi finishes replying, asks a question, or needs tool approval.',
        sound: 'default',
        data: {},
        ...(Platform.OS === 'android' ? { channelId: 'pui-agent-finished' } : {}),
      },
      trigger: null,
    });
    return true;
  } catch {
    return false;
  }
}
