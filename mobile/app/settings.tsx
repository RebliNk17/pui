import React, { useEffect, useMemo, useState } from 'react';
import {
  ActivityIndicator,
  Linking,
  Modal,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Switch,
  Text,
  TextInput,
  View,
} from 'react-native';
import { router } from 'expo-router';
import { Ionicons } from '@expo/vector-icons';
import { CustomScrollbarOverlay, useCustomScrollbar } from '../src/components/CustomScrollbar';
import { modelKey } from '../src/components/ModelEffortSheet';
import type { ModelItem } from '../src/protocol/types';
import { usePui } from '../src/store/connectionStore';
import { usePreferences, type TranscriptViewMode } from '../src/store/preferencesStore';
import {
  DARK_THEMES,
  LIGHT_THEMES,
  usePalette,
  type DarkStyleId,
  type LightStyleId,
  type ThemeMode,
} from '../src/theme/colors';
import {
  getAndroidPackageId,
  getCurrentAppVersion,
  getCurrentVersionCode,
} from '../src/utils/appVersion';
import {
  getNotificationPermissionStatus,
  requestNotificationPermission,
  sendTestNotification,
} from '../src/services/notifications';
import {
  checkForMobileUpdate,
  GITHUB_REPO,
  type UpdateCheckResult,
} from '../src/utils/updates';

const DARK_ORDER: DarkStyleId[] = ['classic', 'midnight', 'forest', 'plum'];
const LIGHT_ORDER: LightStyleId[] = ['classic', 'ivory', 'sage', 'rose'];

export default function SettingsScreen() {
  const colors = usePalette();
  const { prefs, updatePrefs, isFavoriteModel, toggleFavoriteModel } = usePreferences();
  const { activeHost, connectionState, deviceName, setDeviceName, detailsByPath } = usePui();
  const { bindings, metrics } = useCustomScrollbar();

  const [deviceDraft, setDeviceDraft] = useState(deviceName);
  const [favModalOpen, setFavModalOpen] = useState(false);
  const [favQuery, setFavQuery] = useState('');
  const [favProvider, setFavProvider] = useState('All');
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateResult, setUpdateResult] = useState<UpdateCheckResult | null>(null);
  const [notifPerm, setNotifPerm] = useState<'granted' | 'denied' | 'undetermined'>('undetermined');
  const [testSent, setTestSent] = useState<string | null>(null);

  useEffect(() => {
    void getNotificationPermissionStatus().then(setNotifPerm);
  }, []);

  const currentVersion = getCurrentAppVersion();
  const currentVersionCode = getCurrentVersionCode();

  const handleCheckUpdate = async () => {
    setCheckingUpdate(true);
    try {
      const res = await checkForMobileUpdate(currentVersion);
      setUpdateResult(res);
    } finally {
      setCheckingUpdate(false);
    }
  };

  // Collect all unique models reported across any loaded session on the connected host.
  const allModels = useMemo(() => {
    const map = new Map<string, ModelItem>();
    for (const detail of Object.values(detailsByPath)) {
      for (const m of detail.models ?? []) {
        map.set(modelKey(m), m);
      }
    }
    return Array.from(map.values()).sort((a, b) =>
      a.provider.localeCompare(b.provider) || (a.name || a.id).localeCompare(b.name || b.id),
    );
  }, [detailsByPath]);

  const providers = useMemo(() => {
    const set = new Set<string>();
    for (const m of allModels) if (m.provider) set.add(m.provider);
    return ['All', ...Array.from(set).sort()];
  }, [allModels]);

  const filteredModels = useMemo(() => {
    const q = favQuery.trim().toLowerCase();
    return allModels.filter((m) => {
      if (favProvider !== 'All' && m.provider !== favProvider) return false;
      if (!q) return true;
      return (
        m.id.toLowerCase().includes(q) ||
        m.name.toLowerCase().includes(q) ||
        m.provider.toLowerCase().includes(q)
      );
    });
  }, [allModels, favQuery, favProvider]);

  const showingLightSwatches = !colors.dark;

  return (
    <View style={[styles.root, { backgroundColor: colors.bg }]}>
      <ScrollView {...bindings} contentContainerStyle={styles.container}>
        <Text style={[styles.sectionLabel, { color: colors.dim }]}>APPEARANCE</Text>
        <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
          <View style={styles.cardBlock}>
            <Text style={[styles.rowTitle, { color: colors.text, marginBottom: 8 }]}>
              Theme mode
            </Text>
            <View
              style={[
                styles.segment,
                { backgroundColor: colors.bg, borderColor: colors.border },
              ]}
            >
              {(
                [
                  ['system', 'System'],
                  ['dark', 'Dark'],
                  ['light', 'Light'],
                ] as [ThemeMode, string][]
              ).map(([m, label]) => {
                const active = prefs.themeMode === m;
                return (
                  <Pressable
                    key={m}
                    onPress={() => updatePrefs({ themeMode: m })}
                    style={[
                      styles.segBtn,
                      active && { backgroundColor: colors.accentBg },
                    ]}
                  >
                    <Text
                      numberOfLines={1}
                      style={{
                        color: active ? colors.accent : colors.muted,
                        fontSize: 12.5,
                        fontWeight: '600',
                      }}
                    >
                      {label}
                    </Text>
                  </Pressable>
                );
              })}
            </View>
          </View>

          <View style={[styles.divider, { backgroundColor: colors.hairline }]} />

          <View style={styles.cardBlock}>
            <Text style={[styles.rowTitle, { color: colors.text, marginBottom: 8 }]}>
              {showingLightSwatches ? 'Light color style' : 'Dark color style'}
            </Text>
            <View style={styles.swatchGrid}>
              {showingLightSwatches
                ? LIGHT_ORDER.map((id) => {
                    const p = LIGHT_THEMES[id];
                    const active = prefs.lightStyle === id;
                    return (
                      <Pressable
                        key={id}
                        onPress={() => updatePrefs({ lightStyle: id })}
                        style={[
                          styles.swatch,
                          {
                            backgroundColor: active ? colors.accentBg : colors.bg,
                            borderColor: active ? colors.accent : colors.border,
                          },
                        ]}
                      >
                        <View style={[styles.swatchDot, { backgroundColor: p.bg, borderColor: p.border }]}>
                          <View style={[styles.swatchHalf, { backgroundColor: p.accent }]} />
                        </View>
                        <Text
                          numberOfLines={1}
                          style={{ color: colors.text, fontSize: 13, fontWeight: '600' }}
                        >
                          {p.name}
                        </Text>
                      </Pressable>
                    );
                  })
                : DARK_ORDER.map((id) => {
                    const p = DARK_THEMES[id];
                    const active = prefs.darkStyle === id;
                    return (
                      <Pressable
                        key={id}
                        onPress={() => updatePrefs({ darkStyle: id })}
                        style={[
                          styles.swatch,
                          {
                            backgroundColor: active ? colors.accentBg : colors.bg,
                            borderColor: active ? colors.accent : colors.border,
                          },
                        ]}
                      >
                        <View style={[styles.swatchDot, { backgroundColor: p.bg, borderColor: p.border }]}>
                          <View style={[styles.swatchHalf, { backgroundColor: p.accent }]} />
                        </View>
                        <Text
                          numberOfLines={1}
                          style={{ color: colors.text, fontSize: 13, fontWeight: '600' }}
                        >
                          {p.name}
                        </Text>
                      </Pressable>
                    );
                  })}
            </View>
          </View>
        </View>

        <Text style={[styles.sectionLabel, { color: colors.dim }]}>TRANSCRIPT</Text>
        <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
          <View style={styles.cardBlock}>
            <Text style={[styles.rowTitle, { color: colors.text }]}>Transcript view</Text>
            <Text style={[styles.rowSub, { color: colors.muted, marginBottom: 8 }]}>
              Normal collapses reasoning &amp; tools into short summaries. Verbose expands every step and output.
            </Text>
            <View
              style={[
                styles.segment,
                { backgroundColor: colors.bg, borderColor: colors.border },
              ]}
            >
              {(
                [
                  ['normal', 'Normal'],
                  ['verbose', 'Verbose'],
                ] as [TranscriptViewMode, string][]
              ).map(([mode, label]) => {
                const active = prefs.transcriptView === mode;
                return (
                  <Pressable
                    key={mode}
                    onPress={() => updatePrefs({ transcriptView: mode })}
                    style={[styles.segBtn, active && { backgroundColor: colors.accentBg }]}
                  >
                    <Text
                      numberOfLines={1}
                      style={{
                        color: active ? colors.accent : colors.muted,
                        fontSize: 12.5,
                        fontWeight: '600',
                      }}
                    >
                      {label}
                    </Text>
                  </Pressable>
                );
              })}
            </View>
          </View>
        </View>

        <Text style={[styles.sectionLabel, { color: colors.dim }]}>NOTIFICATIONS</Text>
        <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
          <View style={styles.row}>
            <View style={{ flex: 1 }}>
              <Text numberOfLines={1} style={[styles.rowTitle, { color: colors.text }]}>
                Agent notifications
              </Text>
              <Text numberOfLines={2} style={[styles.rowSub, { color: colors.muted }]}>
                {notifPerm === 'granted'
                  ? 'Live alerts over Tailscale (permission granted)'
                  : notifPerm === 'denied'
                    ? 'Blocked in Android settings — tap Test to re-check or open system settings'
                    : 'Tap toggle to allow Android notifications'}
              </Text>
            </View>
            <Switch
              value={prefs.notificationsEnabled}
              onValueChange={async (val) => {
                updatePrefs({ notificationsEnabled: val });
                if (val) {
                  const st = await requestNotificationPermission();
                  setNotifPerm(st);
                }
              }}
              trackColor={{ false: colors.surfaceElevated, true: colors.accent }}
            />
          </View>

          {prefs.notificationsEnabled ? (
            <>
              <View style={[styles.divider, { backgroundColor: colors.hairline }]} />
              <View style={styles.row}>
                <Text style={[styles.rowTitle, { color: colors.text, flex: 1 }]}>
                  Agent finishes replying
                </Text>
                <Switch
                  value={prefs.notifyFinished}
                  onValueChange={(val) => updatePrefs({ notifyFinished: val })}
                  trackColor={{ false: colors.surfaceElevated, true: colors.accent }}
                />
              </View>

              <View style={[styles.divider, { backgroundColor: colors.hairline }]} />
              <View style={styles.row}>
                <Text style={[styles.rowTitle, { color: colors.text, flex: 1 }]}>
                  Tool approval needed
                </Text>
                <Switch
                  value={prefs.notifyApprovals}
                  onValueChange={(val) => updatePrefs({ notifyApprovals: val })}
                  trackColor={{ false: colors.surfaceElevated, true: colors.accent }}
                />
              </View>

              <View style={[styles.divider, { backgroundColor: colors.hairline }]} />
              <View style={styles.row}>
                <Text style={[styles.rowTitle, { color: colors.text, flex: 1 }]}>
                  Agent asks questions
                </Text>
                <Switch
                  value={prefs.notifyQuestions}
                  onValueChange={(val) => updatePrefs({ notifyQuestions: val })}
                  trackColor={{ false: colors.surfaceElevated, true: colors.accent }}
                />
              </View>

              {Platform.OS === 'android' ? (
                <>
                  <View style={[styles.divider, { backgroundColor: colors.hairline }]} />
                  <View style={styles.row}>
                    <View style={{ flex: 1 }}>
                      <Text numberOfLines={1} style={[styles.rowTitle, { color: colors.text }]}>
                        Keep alive in background
                      </Text>
                      <Text numberOfLines={2} style={[styles.rowSub, { color: colors.muted }]}>
                        Shows a quiet Android status notification so alerts arrive even with screen off.
                      </Text>
                    </View>
                    <Switch
                      value={prefs.keepAliveInBackground}
                      onValueChange={(val) => updatePrefs({ keepAliveInBackground: val })}
                      trackColor={{ false: colors.surfaceElevated, true: colors.accent }}
                    />
                  </View>
                </>
              ) : null}

              <View style={[styles.divider, { backgroundColor: colors.hairline }]} />
              <View style={styles.row}>
                <Text style={[styles.rowSub, { color: colors.muted, flex: 1 }]}>
                  {testSent ?? 'Verify sound & banner delivery on this device.'}
                </Text>
                <Pressable
                  onPress={async () => {
                    const ok = await sendTestNotification();
                    const st = await getNotificationPermissionStatus();
                    setNotifPerm(st);
                    setTestSent(
                      ok
                        ? 'Test notification sent!'
                        : 'Permission not granted — enable in Android App Settings.',
                    );
                  }}
                  style={[
                    styles.saveBtn,
                    { backgroundColor: colors.surfaceElevated, borderColor: colors.border },
                  ]}
                >
                  <Text style={{ color: colors.text, fontSize: 12.5, fontWeight: '600' }}>
                    Send test
                  </Text>
                </Pressable>
              </View>
            </>
          ) : null}
        </View>

        <Text style={[styles.sectionLabel, { color: colors.dim }]}>MODELS</Text>
        <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
          <Pressable onPress={() => setFavModalOpen(true)} style={styles.row}>
            <View style={{ flex: 1 }}>
              <Text numberOfLines={1} style={[styles.rowTitle, { color: colors.text }]}>
                Favorite / pinned models
              </Text>
              <Text numberOfLines={1} style={[styles.rowSub, { color: colors.muted }]}>
                {prefs.favoriteModels.length === 0
                  ? 'Star models to pin them at the top of the picker'
                  : `${prefs.favoriteModels.length} pinned · all models remain searchable`}
              </Text>
            </View>
            <View style={styles.rowRight}>
              <Text style={{ color: colors.muted, fontSize: 13 }}>Manage</Text>
              <Ionicons name="chevron-forward" size={16} color={colors.dim} />
            </View>
          </Pressable>
        </View>

        <Text style={[styles.sectionLabel, { color: colors.dim }]}>HOST &amp; DEVICE</Text>
        <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
          <Pressable onPress={() => router.push('/connect')} style={styles.row}>
            <View style={{ flex: 1 }}>
              <Text numberOfLines={1} style={[styles.rowTitle, { color: colors.text }]}>
                {activeHost ? activeHost.name : 'No host connected'}
              </Text>
              <Text numberOfLines={1} style={[styles.rowSub, { color: colors.muted }]}>
                {activeHost
                  ? `${activeHost.host}:${activeHost.port} · ${connectionState}`
                  : 'Pair via QR code or Tailscale IP'}
              </Text>
            </View>
            <View style={styles.rowRight}>
              <Text
                style={{
                  color: connectionState === 'connected' ? colors.green : colors.muted,
                  fontSize: 12,
                  fontWeight: '600',
                }}
              >
                {connectionState === 'connected' ? 'Live' : 'Switch'}
              </Text>
              <Ionicons name="chevron-forward" size={16} color={colors.dim} />
            </View>
          </Pressable>

          <View style={[styles.divider, { backgroundColor: colors.hairline }]} />

          <View style={styles.cardBlock}>
            <Text style={[styles.rowTitle, { color: colors.text }]}>This device name</Text>
            <Text style={[styles.rowSub, { color: colors.muted, marginBottom: 8 }]}>
              Tagged as the origin on sessions started from this phone.
            </Text>
            <View style={styles.deviceRow}>
              <TextInput
                value={deviceDraft}
                onChangeText={setDeviceDraft}
                placeholder="S26 Ultra"
                placeholderTextColor={colors.dim}
                style={[
                  styles.input,
                  {
                    backgroundColor: colors.bg,
                    color: colors.text,
                    borderColor: colors.border,
                  },
                ]}
              />
              <Pressable
                onPress={() => void setDeviceName(deviceDraft)}
                style={[
                  styles.saveBtn,
                  { backgroundColor: colors.accentBg, borderColor: colors.accent },
                ]}
              >
                <Text style={{ color: colors.accent, fontSize: 13, fontWeight: '600' }}>Save</Text>
              </Pressable>
            </View>
          </View>
        </View>

        <Text style={[styles.sectionLabel, { color: colors.dim }]}>APP &amp; UPDATES</Text>
        <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
          <View style={styles.row}>
            <View style={{ flex: 1 }}>
              <Text numberOfLines={1} style={[styles.rowTitle, { color: colors.text }]}>
                Pui Mobile v{currentVersion}
                {currentVersionCode ? ` (${currentVersionCode})` : ''}
              </Text>
              <Text numberOfLines={2} style={[styles.rowSub, { color: colors.muted }]}>
                {updateResult?.error
                  ? updateResult.error
                  : updateResult?.updateAvailable && updateResult.latest
                    ? `Update available: v${updateResult.latest.version}`
                    : updateResult
                      ? 'You are on the latest version'
                      : getAndroidPackageId()}
              </Text>
            </View>
            <Pressable
              onPress={() => void handleCheckUpdate()}
              disabled={checkingUpdate}
              style={[
                styles.saveBtn,
                { backgroundColor: colors.surfaceElevated, borderColor: colors.border },
              ]}
            >
              {checkingUpdate ? (
                <ActivityIndicator size="small" color={colors.accent} />
              ) : (
                <Text style={{ color: colors.text, fontSize: 12.5, fontWeight: '600' }}>Check</Text>
              )}
            </Pressable>
          </View>

          {updateResult?.updateAvailable && updateResult.latest ? (
            <>
              <View style={[styles.divider, { backgroundColor: colors.hairline }]} />
              <View style={[styles.cardBlock, { flexDirection: 'row', gap: 8 }]}>
                <Pressable
                  onPress={() =>
                    void Linking.openURL(
                      updateResult.latest?.apkUrl || updateResult.latest?.releaseUrl || '',
                    )
                  }
                  style={[
                    styles.saveBtn,
                    {
                      flex: 1,
                      backgroundColor: colors.accent,
                      borderColor: colors.accent,
                      alignItems: 'center',
                    },
                  ]}
                >
                  <Text style={{ color: '#fff', fontSize: 13, fontWeight: '700' }}>
                    Download APK (v{updateResult.latest.version})
                  </Text>
                </Pressable>
              </View>
            </>
          ) : (
            <>
              <View style={[styles.divider, { backgroundColor: colors.hairline }]} />
              <Pressable
                onPress={() =>
                  void Linking.openURL(`https://github.com/${GITHUB_REPO}/releases`)
                }
                style={styles.row}
              >
                <Text style={[styles.rowTitle, { color: colors.text }]}>
                  View GitHub Releases
                </Text>
                <Ionicons name="open-outline" size={16} color={colors.dim} />
              </Pressable>
            </>
          )}
        </View>
      </ScrollView>
      <CustomScrollbarOverlay {...metrics} />

      <Modal
        visible={favModalOpen}
        animationType="slide"
        onRequestClose={() => setFavModalOpen(false)}
      >
        <View style={[styles.modalRoot, { backgroundColor: colors.bg }]}>
          <View
            style={[
              styles.modalHead,
              { backgroundColor: colors.surface, borderBottomColor: colors.border },
            ]}
          >
            <Text style={[styles.modalTitle, { color: colors.text }]}>Favorite models</Text>
            <Pressable onPress={() => setFavModalOpen(false)} style={{ padding: 6 }}>
              <Ionicons name="close" size={22} color={colors.muted} />
            </Pressable>
          </View>

          <View style={{ padding: 14, gap: 8 }}>
            <View
              style={[
                styles.searchBox,
                { backgroundColor: colors.surface, borderColor: colors.border },
              ]}
            >
              <Ionicons name="search-outline" size={16} color={colors.dim} />
              <TextInput
                value={favQuery}
                onChangeText={setFavQuery}
                placeholder="Search models or providers…"
                placeholderTextColor={colors.dim}
                style={{ flex: 1, color: colors.text, fontSize: 13.5, textAlign: 'left' }}
              />
            </View>

            {providers.length > 2 ? (
              <ScrollView
                horizontal
                showsHorizontalScrollIndicator={false}
                contentContainerStyle={{ gap: 6 }}
              >
                {providers.map((p) => {
                  const active = p === favProvider;
                  return (
                    <Pressable
                      key={p}
                      onPress={() => setFavProvider(p)}
                      style={[
                        styles.chip,
                        {
                          backgroundColor: active ? colors.accentBg : colors.surface,
                          borderColor: active ? colors.accent : colors.border,
                        },
                      ]}
                    >
                      <Text
                        style={{
                          color: active ? colors.accent : colors.muted,
                          fontSize: 12,
                          fontWeight: '600',
                        }}
                      >
                        {p}
                      </Text>
                    </Pressable>
                  );
                })}
              </ScrollView>
            ) : null}
          </View>

          <ScrollView contentContainerStyle={{ paddingHorizontal: 14, paddingBottom: 30 }}>
            {filteredModels.length === 0 ? (
              <Text style={{ color: colors.muted, textAlign: 'center', padding: 24, fontSize: 13 }}>
                {allModels.length === 0
                  ? 'Open any chat first so Pui loads your configured provider models, or star models directly from the chat model picker.'
                  : 'No models match your search.'}
              </Text>
            ) : (
              <View
                style={[
                  styles.card,
                  { backgroundColor: colors.surface, borderColor: colors.border },
                ]}
              >
                {filteredModels.map((m, idx) => {
                  const key = modelKey(m);
                  const fav = isFavoriteModel(key);
                  return (
                    <Pressable
                      key={key}
                      onPress={() => toggleFavoriteModel(key)}
                      style={[
                        styles.row,
                        idx > 0 && {
                          borderTopWidth: StyleSheet.hairlineWidth,
                          borderTopColor: colors.hairline,
                        },
                      ]}
                    >
                      <View style={{ flex: 1 }}>
                        <Text numberOfLines={1} style={[styles.rowTitle, { color: colors.text }]}>
                          {m.name || m.id}
                        </Text>
                        <Text numberOfLines={1} style={[styles.rowSub, { color: colors.muted }]}>
                          {m.provider}
                        </Text>
                      </View>
                      <Ionicons
                        name={fav ? 'star' : 'star-outline'}
                        size={20}
                        color={fav ? colors.amber : colors.dim}
                      />
                    </Pressable>
                  );
                })}
              </View>
            )}
          </ScrollView>
        </View>
      </Modal>
    </View>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
    direction: 'ltr',
  },
  container: {
    padding: 14,
    paddingBottom: 36,
  },
  sectionLabel: {
    fontSize: 11,
    fontWeight: '700',
    letterSpacing: 0.7,
    marginTop: 14,
    marginBottom: 6,
    marginLeft: 4,
    textAlign: 'left',
  },
  card: {
    borderRadius: 14,
    borderWidth: 1,
    overflow: 'hidden',
  },
  cardBlock: {
    padding: 14,
  },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 10,
    paddingHorizontal: 14,
    paddingVertical: 13,
  },
  rowRight: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 4,
    flexShrink: 0,
  },
  rowTitle: {
    fontSize: 14,
    fontWeight: '600',
    textAlign: 'left',
  },
  rowSub: {
    fontSize: 12,
    marginTop: 2,
    textAlign: 'left',
  },
  divider: {
    height: StyleSheet.hairlineWidth,
  },
  segment: {
    flexDirection: 'row',
    borderWidth: 1,
    borderRadius: 10,
    padding: 3,
    gap: 3,
  },
  segBtn: {
    flex: 1,
    alignItems: 'center',
    paddingVertical: 7,
    borderRadius: 7,
  },
  swatchGrid: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    gap: 8,
  },
  swatch: {
    width: '48%',
    flexDirection: 'row',
    alignItems: 'center',
    gap: 9,
    paddingHorizontal: 10,
    paddingVertical: 10,
    borderRadius: 10,
    borderWidth: 1.5,
  },
  swatchDot: {
    width: 20,
    height: 20,
    borderRadius: 10,
    borderWidth: 1,
    overflow: 'hidden',
  },
  swatchHalf: {
    position: 'absolute',
    right: 0,
    top: 0,
    bottom: 0,
    width: 10,
  },
  deviceRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  input: {
    flex: 1,
    borderWidth: 1,
    borderRadius: 10,
    paddingHorizontal: 12,
    paddingVertical: 9,
    fontSize: 13.5,
    textAlign: 'left',
  },
  saveBtn: {
    borderWidth: 1,
    borderRadius: 10,
    paddingHorizontal: 14,
    paddingVertical: 9,
  },
  modalRoot: {
    flex: 1,
    paddingTop: 44,
    direction: 'ltr',
  },
  modalHead: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 16,
    paddingVertical: 12,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  modalTitle: {
    fontSize: 16,
    fontWeight: '700',
  },
  searchBox: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    borderWidth: 1,
    borderRadius: 10,
    paddingHorizontal: 11,
    height: 40,
  },
  chip: {
    paddingHorizontal: 11,
    paddingVertical: 5,
    borderRadius: 999,
    borderWidth: 1,
  },
});
