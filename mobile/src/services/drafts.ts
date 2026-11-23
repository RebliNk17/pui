import { Platform } from 'react-native';
import * as SecureStore from 'expo-secure-store';
import { DRAFTS_STORAGE_KEY, parseDraftMap, upsertDraft, type DraftMap } from '../utils/drafts';

let cache: DraftMap | null = null;

async function readAll(): Promise<DraftMap> {
  if (cache) return cache;
  try {
    const raw =
      Platform.OS === 'web'
        ? typeof localStorage !== 'undefined'
          ? localStorage.getItem(DRAFTS_STORAGE_KEY)
          : null
        : await SecureStore.getItemAsync(DRAFTS_STORAGE_KEY);
    cache = parseDraftMap(raw);
    return cache;
  } catch {
    cache = {};
    return cache;
  }
}

async function writeAll(next: DraftMap): Promise<void> {
  cache = next;
  const serialized = JSON.stringify(next);
  try {
    if (Platform.OS === 'web') {
      if (typeof localStorage !== 'undefined') localStorage.setItem(DRAFTS_STORAGE_KEY, serialized);
    } else {
      await SecureStore.setItemAsync(DRAFTS_STORAGE_KEY, serialized);
    }
  } catch {
    // ignore storage quota errors
  }
}

export async function loadChatDraft(sessionPath: string): Promise<string> {
  if (!sessionPath) return '';
  const all = await readAll();
  return all[sessionPath]?.text ?? '';
}

export async function saveChatDraft(sessionPath: string, text: string): Promise<void> {
  if (!sessionPath) return;
  const all = await readAll();
  const next = upsertDraft(all, sessionPath, text);
  await writeAll(next);
}

export async function clearChatDraft(sessionPath: string): Promise<void> {
  await saveChatDraft(sessionPath, '');
}
