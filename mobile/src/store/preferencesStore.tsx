import React, { createContext, useCallback, useContext, useEffect, useState } from 'react';
import { Platform } from 'react-native';
import * as SecureStore from 'expo-secure-store';
import {
  ThemeSelectionProvider,
  type DarkStyleId,
  type LightStyleId,
  type ThemeMode,
} from '../theme/colors';
import type { SessionGrouping, SessionOrdering } from '../utils/sessions';

const PREFS_KEY = 'pui_mobile_prefs_v1';

export type TranscriptViewMode = 'normal' | 'verbose';

export interface MobilePreferences {
  themeMode: ThemeMode;
  darkStyle: DarkStyleId;
  lightStyle: LightStyleId;
  favoriteModels: string[];
  defaultGrouping: SessionGrouping;
  defaultOrdering: SessionOrdering;
  defaultEffort: string;
  transcriptView: TranscriptViewMode;
  notificationsEnabled: boolean;
  notifyFinished: boolean;
  notifyApprovals: boolean;
  notifyQuestions: boolean;
  keepAliveInBackground: boolean;
}

const DEFAULT_PREFS: MobilePreferences = {
  themeMode: 'dark',
  darkStyle: 'classic',
  lightStyle: 'classic',
  favoriteModels: [],
  defaultGrouping: 'date',
  defaultOrdering: 'recent',
  defaultEffort: 'medium',
  transcriptView: 'normal',
  notificationsEnabled: true,
  notifyFinished: true,
  notifyApprovals: true,
  notifyQuestions: true,
  keepAliveInBackground: true,
};

async function readPrefs(): Promise<MobilePreferences> {
  try {
    const raw =
      Platform.OS === 'web'
        ? typeof localStorage !== 'undefined'
          ? localStorage.getItem(PREFS_KEY)
          : null
        : await SecureStore.getItemAsync(PREFS_KEY);
    if (!raw) return DEFAULT_PREFS;
    const parsed = JSON.parse(raw) as Partial<MobilePreferences>;
    return {
      ...DEFAULT_PREFS,
      ...parsed,
      favoriteModels: Array.isArray(parsed.favoriteModels) ? parsed.favoriteModels : [],
    };
  } catch {
    return DEFAULT_PREFS;
  }
}

async function writePrefs(prefs: MobilePreferences): Promise<void> {
  const raw = JSON.stringify(prefs);
  try {
    if (Platform.OS === 'web') {
      if (typeof localStorage !== 'undefined') localStorage.setItem(PREFS_KEY, raw);
    } else {
      await SecureStore.setItemAsync(PREFS_KEY, raw);
    }
  } catch {
    // ignore
  }
}

export interface PreferencesContextValue {
  prefs: MobilePreferences;
  updatePrefs: (patch: Partial<MobilePreferences>) => void;
  toggleFavoriteModel: (modelKey: string) => void;
  isFavoriteModel: (modelKey: string) => boolean;
}

const PreferencesContext = createContext<PreferencesContextValue | null>(null);

export function PreferencesProvider({ children }: { children: React.ReactNode }) {
  const [prefs, setPrefs] = useState<MobilePreferences>(DEFAULT_PREFS);

  useEffect(() => {
    let cancelled = false;
    void readPrefs().then((loaded) => {
      if (!cancelled) setPrefs(loaded);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const updatePrefs = useCallback((patch: Partial<MobilePreferences>) => {
    setPrefs((prev) => {
      const next = { ...prev, ...patch };
      void writePrefs(next);
      return next;
    });
  }, []);

  const toggleFavoriteModel = useCallback((modelKey: string) => {
    setPrefs((prev) => {
      const exists = prev.favoriteModels.includes(modelKey);
      const favoriteModels = exists
        ? prev.favoriteModels.filter((k) => k !== modelKey)
        : [modelKey, ...prev.favoriteModels];
      const next = { ...prev, favoriteModels };
      void writePrefs(next);
      return next;
    });
  }, []);

  const isFavoriteModel = useCallback(
    (modelKey: string) => prefs.favoriteModels.includes(modelKey),
    [prefs.favoriteModels],
  );

  return (
    <PreferencesContext.Provider
      value={{ prefs, updatePrefs, toggleFavoriteModel, isFavoriteModel }}
    >
      <ThemeSelectionProvider
        selection={{
          mode: prefs.themeMode,
          darkStyle: prefs.darkStyle,
          lightStyle: prefs.lightStyle,
        }}
      >
        {children}
      </ThemeSelectionProvider>
    </PreferencesContext.Provider>
  );
}

export function usePreferences(): PreferencesContextValue {
  const ctx = useContext(PreferencesContext);
  if (!ctx) throw new Error('usePreferences must be used within PreferencesProvider');
  return ctx;
}
