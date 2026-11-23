import { NativeModules, Platform } from 'react-native';

export interface SharedPayload {
  text?: string;
  imageBase64?: string;
  imageMime?: string;
}

interface ShareNativeModule {
  getPendingShare(): Promise<SharedPayload | null>;
  clearPendingShare(): void;
}

const nativeMod: ShareNativeModule | undefined =
  Platform.OS === 'android' ? NativeModules.PuiShare : undefined;

export async function consumePendingShare(): Promise<SharedPayload | null> {
  if (!nativeMod || typeof nativeMod.getPendingShare !== 'function') return null;
  try {
    const data = await nativeMod.getPendingShare();
    if (!data || (!data.text && !data.imageBase64)) return null;
    nativeMod.clearPendingShare();
    return data;
  } catch {
    return null;
  }
}
