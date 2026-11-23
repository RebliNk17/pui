import React, { useEffect } from 'react';
import { I18nManager, Platform, StyleSheet, View } from 'react-native';
import * as Notifications from 'expo-notifications';
import { router, Stack } from 'expo-router';
import { StatusBar } from 'expo-status-bar';
import { SafeAreaProvider } from 'react-native-safe-area-context';
import { initNotifications, requestNotificationPermission } from '../src/services/notifications';
import { PuiProvider } from '../src/store/connectionStore';
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

  useEffect(() => {
    if (Platform.OS === 'web') return;
    void initNotifications();
    if (prefs.notificationsEnabled) {
      void requestNotificationPermission();
    }
  }, [prefs.notificationsEnabled]);

  useEffect(() => {
    if (Platform.OS === 'web') return;
    const openFromResponse = (response: Notifications.NotificationResponse | null) => {
      const path = response?.notification?.request?.content?.data?.path;
      if (typeof path === 'string' && path.trim()) {
        router.push({
          pathname: '/chat/[id]',
          params: { id: encodeURIComponent(path) },
        });
      }
    };

    void Notifications.getLastNotificationResponseAsync().then(openFromResponse).catch(() => {});
    const sub = Notifications.addNotificationResponseReceivedListener(openFromResponse);
    return () => sub.remove();
  }, []);

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
