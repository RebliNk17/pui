import React, { useEffect } from 'react';
import { AppState, I18nManager, Platform, StyleSheet, View } from 'react-native';
import * as Notifications from 'expo-notifications';
import { router, Stack } from 'expo-router';
import { StatusBar } from 'expo-status-bar';
import { SafeAreaProvider } from 'react-native-safe-area-context';
import {
  ACTION_APPROVE,
  ACTION_DENY,
  initNotifications,
  requestNotificationPermission,
} from '../src/services/notifications';
import { consumePendingShare } from '../src/services/shareIntent';
import { PuiProvider, usePui } from '../src/store/connectionStore';
import { PreferencesProvider, usePreferences } from '../src/store/preferencesStore';
import { usePalette } from '../src/theme/colors';

// Lock UI chrome to LTR so Hebrew/Arabic OS settings do not mirror navigation, tabs,
// or toolbars; individual Hebrew/Arabic messages still render RTL per paragraph.
try {
  if (I18nManager.isRTL) {
    I18nManager.allowRTL(false);
    I18nManager.forceRTL(false);
  }
} catch {
  // Ignore on web or restricted environments.
}

function NavigationStack() {
  const colors = usePalette();
  const { prefs } = usePreferences();
  const { transport, setPendingShare } = usePui();

  useEffect(() => {
    if (Platform.OS === 'web') return;
    void initNotifications();
    if (prefs.notificationsEnabled) {
      void requestNotificationPermission();
    }
  }, [prefs.notificationsEnabled]);

  // Capture Android Share Sheet ("Share to Pui") payloads on launch & foreground resume.
  useEffect(() => {
    if (Platform.OS !== 'android') return;
    const checkShare = () => {
      void consumePendingShare().then((payload) => {
        if (payload) {
          setPendingShare(payload);
        }
      });
    };
    checkShare();
    const sub = AppState.addEventListener('change', (state) => {
      if (state === 'active') checkShare();
    });
    return () => sub.remove();
  }, [setPendingShare]);

  useEffect(() => {
    if (Platform.OS === 'web') return;
    const handleResponse = (response: Notifications.NotificationResponse | null) => {
      if (!response) return;
      const data = response.notification?.request?.content?.data ?? {};
      const path = typeof data.path === 'string' ? data.path : '';
      const dialogId = typeof data.dialogId === 'string' ? data.dialogId : '';
      const dialogMethod = typeof data.dialogMethod === 'string' ? data.dialogMethod : 'confirm';
      const action = response.actionIdentifier;

      // Lock-screen / notification-shade quick Approve or Deny without opening the chat!
      if ((action === ACTION_APPROVE || action === ACTION_DENY) && dialogId && transport) {
        const allow = action === ACTION_APPROVE;
        const payload =
          dialogMethod === 'select'
            ? { value: allow ? 'Allow once' : 'Deny' }
            : { confirmed: allow };
        void transport
          .answerDialog({
            path: path || undefined,
            dialog_id: dialogId,
            response: payload,
          })
          .catch(() => {});
        return;
      }

      if (path.trim()) {
        router.push({
          pathname: '/chat/[id]',
          params: { id: encodeURIComponent(path) },
        });
      }
    };

    void Notifications.getLastNotificationResponseAsync().then(handleResponse).catch(() => {});
    const sub = Notifications.addNotificationResponseReceivedListener(handleResponse);
    return () => sub.remove();
  }, [transport]);

  return (
    <View style={[styles.root, { backgroundColor: colors.bg }]}>
      <StatusBar style={colors.dark ? 'light' : 'dark'} />
      <Stack
        screenOptions={{
          headerStyle: { backgroundColor: colors.surface },
          headerTintColor: colors.text,
          headerShadowVisible: false,
          contentStyle: { backgroundColor: colors.bg },
        }}
      >
        <Stack.Screen name="index" options={{ headerShown: false }} />
        <Stack.Screen name="connect" options={{ title: 'Connect to Pui' }} />
        <Stack.Screen name="settings" options={{ title: 'Settings' }} />
        <Stack.Screen name="chat/[id]" options={{ headerShown: false }} />
      </Stack>
    </View>
  );
}

export default function RootLayout() {
  return (
    <SafeAreaProvider>
      <PreferencesProvider>
        <PuiProvider>
          <NavigationStack />
        </PuiProvider>
      </PreferencesProvider>
    </SafeAreaProvider>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
    direction: 'ltr',
  },
});
