import { NativeModules, Platform } from 'react-native';

interface KeepAliveNativeModule {
  start(hostName: string): void;
  stop(): void;
}

const nativeMod: KeepAliveNativeModule | undefined =
  Platform.OS === 'android' ? NativeModules.PuiKeepAlive : undefined;

export function isKeepAliveSupported(): boolean {
  return Boolean(nativeMod && typeof nativeMod.start === 'function');
}

export function startBackgroundKeepAlive(hostName: string): void {
  try {
    nativeMod?.start(hostName || 'Pui Desktop');
  } catch {
    // No-op in Expo Go or when Android foreground service permission is unavailable.
  }
}

export function stopBackgroundKeepAlive(): void {
  try {
    nativeMod?.stop();
  } catch {
    // ignore
  }
}
