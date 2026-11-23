import React from 'react';
import { Stack } from 'expo-router';
import { StatusBar } from 'expo-status-bar';
import { SafeAreaProvider } from 'react-native-safe-area-context';
import { PuiProvider } from '../src/store/connectionStore';
import { usePalette } from '../src/theme/colors';

function NavigationStack() {
  const colors = usePalette();
  return (
    <>
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
        <Stack.Screen name="chat/[id]" options={{ headerShown: false }} />
      </Stack>
    </>
  );
}

export default function RootLayout() {
  return (
    <SafeAreaProvider>
      <PuiProvider>
        <NavigationStack />
      </PuiProvider>
    </SafeAreaProvider>
  );
}
