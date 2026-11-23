import React, { useEffect, useMemo, useState } from 'react';
import {
  Linking,
  Modal,
  Pressable,
  RefreshControl,
  SectionList,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { router } from 'expo-router';
import { Ionicons } from '@expo/vector-icons';
import { CustomScrollbarOverlay, useCustomScrollbar } from '../src/components/CustomScrollbar';
import { NewSessionModal } from '../src/components/NewSessionModal';
import { UsageSheet } from '../src/components/UsageSheet';
import type { SessionSummary } from '../src/protocol/types';
import { usePui } from '../src/store/connectionStore';
import { usePreferences } from '../src/store/preferencesStore';
import { usePalette } from '../src/theme/colors';
import { isRtlText } from '../src/utils/markdown';
import {
  childrenOfSession,
  filterSessions,
  formatRelativeAge,
  groupSessions,
  type SessionFilter,
  type SessionGrouping,
  type SessionOrdering,
} from '../src/utils/sessions';
import { getCurrentAppVersion } from '../src/utils/appVersion';
import { checkForMobileUpdate, type ReleaseInfo } from '../src/utils/updates';

export default function SessionsScreen() {
  const colors = usePalette();
  const { prefs, updatePrefs } = usePreferences();
  const {
    activeHost,
    connectionState,
    connectionError,
    hostInfo,
    sessions,
    transport,
    clientOrigin,
    deviceName,
    retryConnection,
    refreshSessions,
    pendingShare,
    setPendingShare,
  } = usePui();

  const [query, setQuery] = useState('');
  const [searchOpen, setSearchOpen] = useState(false);
  const [filter, setFilter] = useState<SessionFilter>('all');
  const [sortModalOpen, setSortModalOpen] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [newOpen, setNewOpen] = useState(false);
  const [usageOpen, setUsageOpen] = useState(false);
  const [availableUpdate, setAvailableUpdate] = useState<ReleaseInfo | null>(null);
  const [updateDismissed, setUpdateDismissed] = useState(false);
  const [actionTarget, setActionTarget] = useState<SessionSummary | null>(null);
  const [renameMode, setRenameMode] = useState(false);
  const [renameDraft, setRenameDraft] = useState('');
  const [confirmDelete, setConfirmDelete] = useState(false);
  const { bindings, metrics } = useCustomScrollbar();

  useEffect(() => {
    let cancelled = false;
    checkForMobileUpdate(getCurrentAppVersion())
      .then((res) => {
        if (!cancelled && res.updateAvailable && res.latest) {
          setAvailableUpdate(res.latest);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  const grouping = prefs.defaultGrouping;
  const ordering = prefs.defaultOrdering;

  const sections = useMemo(() => {
    const filtered = filterSessions(sessions, filter, query);
    return groupSessions(filtered, grouping, ordering);
  }, [sessions, filter, query, grouping, ordering]);

  const onRefresh = async () => {
    setRefreshing(true);
    try {
      await refreshSessions();
    } finally {
      setRefreshing(false);
    }
  };

  const openSession = (session: SessionSummary) => {
    // Loads the session in background on host without switching Desktop's active chat.
    void transport?.openSession(session.path).catch(() => {});
    router.push({
      pathname: '/chat/[id]',
      params: { id: encodeURIComponent(session.path) },
    });
  };

  const statusDot =
    connectionState === 'connected'
      ? colors.green
      : connectionState === 'connecting'
        ? colors.amber
        : colors.red;

  const defaultPermissionModes = hostInfo?.permission_modes ?? [
    { key: 'auto', label: 'Auto' },
    { key: 'manual', label: 'Manual' },
    { key: 'acceptEdits', label: 'Accept edits' },
    { key: 'plan', label: 'Plan' },
    { key: 'dontAsk', label: "Don't ask" },
    { key: 'bypassPermissions', label: 'Bypass' },
  ];

  const renderSessionRow = ({ item }: { item: SessionSummary }) => {
    const isPhone = item.origin.platform === 'android' || item.origin.platform === 'ios';
    const isChild = Boolean(item.parent_session);
    const childCount = childrenOfSession(sessions, item.path).length;
    const dotColor = item.has_pending_dialog
      ? colors.amber
      : item.busy
        ? colors.accent
        : colors.dim;
    const age = formatRelativeAge(item.modified_ms);
    const titleRtl = isRtlText(item.title);

    return (
      <Pressable
        onPress={() => openSession(item)}
        onLongPress={() => {
          setActionTarget(item);
          setRenameMode(false);
          setRenameDraft(item.title);
          setConfirmDelete(false);
        }}
        style={({ pressed }) => [
          styles.sessionRow,
          isChild && { paddingLeft: 28 },
          (pressed || item.has_pending_dialog) && {
            backgroundColor: colors.surface,
          },
        ]}
      >
        {isChild ? (
          <Ionicons name="return-down-forward-outline" size={12} color={colors.dim} />
        ) : null}
        <View style={[styles.rowDot, { backgroundColor: dotColor }]} />
        <View style={styles.rowMiddle}>
          <View style={{ flexDirection: 'row', alignItems: 'center', gap: 5 }}>
            {item.pinned ? (
              <Ionicons name="pin" size={11} color={colors.accent} />
            ) : null}
            <Text
              numberOfLines={1}
              style={[
                styles.rowTitle,
                {
                  flex: 1,
                  color: colors.text,
                  textAlign: titleRtl ? 'right' : 'left',
                },
              ]}
            >
              {item.title}
            </Text>
          </View>
          <View style={styles.rowSubLine}>
            <Text numberOfLines={1} style={[styles.rowMeta, { color: colors.muted }]}>
              {item.project_name}
              {item.busy ? ' · Working…' : item.has_pending_dialog ? ' · Needs approval' : ''}
              {childCount > 0 ? ` · ${childCount} subagent${childCount > 1 ? 's' : ''}` : ''}
            </Text>
            {isPhone ? (
              <View style={[styles.originPill, { backgroundColor: colors.accentBg }]}>
                <Ionicons name="phone-portrait-outline" size={10} color={colors.accent} />
                <Text numberOfLines={1} style={[styles.originPillText, { color: colors.accent }]}>
                  {item.origin.device || (item.origin.platform === 'ios' ? 'iOS' : 'Android')}
                </Text>
              </View>
            ) : null}
          </View>
        </View>
        <Text numberOfLines={1} style={[styles.rowAge, { color: colors.dim }]}>
          {age}
        </Text>
      </Pressable>
    );
  };

  return (
    <SafeAreaView style={[styles.safe, { backgroundColor: colors.bg }]}>
      {/* Top header: Host chip on left, crisp Search + Settings gear on right */}
      <View style={[styles.header, { borderBottomColor: colors.hairline }]}>
        <Pressable
          onPress={() => router.push('/connect')}
          style={[
            styles.hostChip,
            { backgroundColor: colors.surface, borderColor: colors.border },
          ]}
        >
          <View style={[styles.dot, { backgroundColor: statusDot }]} />
          <Text numberOfLines={1} style={[styles.hostTitle, { color: colors.text }]}>
            {activeHost ? activeHost.name : 'Connect to Pui'}
          </Text>
          <Ionicons name="chevron-down" size={14} color={colors.dim} />
        </Pressable>

        <View style={styles.headerActions}>
          {activeHost ? (
            <>
              <Pressable
                onPress={() => setUsageOpen(true)}
                style={styles.headerIconBtn}
                accessibilityLabel="Provider rate limits"
              >
                <Ionicons name="speedometer-outline" size={20} color={colors.muted} />
              </Pressable>
              <Pressable
                onPress={() => {
                  setSearchOpen((v) => {
                    if (v) setQuery('');
                    return !v;
                  });
                }}
                style={[
                  styles.headerIconBtn,
                  searchOpen && { backgroundColor: colors.accentBg },
                ]}
              >
                <Ionicons
                  name={searchOpen ? 'close-outline' : 'search-outline'}
                  size={21}
                  color={searchOpen ? colors.accent : colors.muted}
                />
              </Pressable>
            </>
          ) : null}
          <Pressable
            onPress={() => router.push('/settings')}
            style={styles.headerIconBtn}
          >
            <Ionicons name="settings-outline" size={21} color={colors.muted} />
          </Pressable>
        </View>
      </View>

      {pendingShare ? (
        <View
          style={[
            styles.banner,
            {
              backgroundColor: colors.accentBg,
              borderBottomWidth: StyleSheet.hairlineWidth,
              borderBottomColor: colors.accent,
            },
          ]}
        >
          <Ionicons name="share-outline" size={16} color={colors.accent} />
          <Text numberOfLines={2} style={{ color: colors.text, fontSize: 12, flex: 1, textAlign: 'left' }}>
            {pendingShare.imageBase64
              ? 'Shared photo ready — tap any chat to attach it.'
              : 'Shared text ready — tap any chat to prefill it.'}
          </Text>
          <Pressable onPress={() => setPendingShare(null)} hitSlop={8}>
            <Ionicons name="close" size={16} color={colors.muted} />
          </Pressable>
        </View>
      ) : null}

      {availableUpdate && !updateDismissed ? (
        <View
          style={[
            styles.banner,
            { backgroundColor: colors.accentBg, borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: colors.border },
          ]}
        >
          <Ionicons name="arrow-up-circle-outline" size={16} color={colors.accent} />
          <Text style={{ color: colors.text, fontSize: 12, flex: 1, textAlign: 'left' }}>
            Pui Mobile v{availableUpdate.version} is available
          </Text>
          <Pressable
            onPress={() =>
              void Linking.openURL(availableUpdate.apkUrl || availableUpdate.releaseUrl)
            }
            style={[styles.retryBtn, { backgroundColor: colors.accent }]}
          >
            <Text style={{ color: '#fff', fontSize: 11.5, fontWeight: '700' }}>Update</Text>
          </Pressable>
          <Pressable onPress={() => setUpdateDismissed(true)} hitSlop={8}>
            <Ionicons name="close" size={16} color={colors.muted} />
          </Pressable>
        </View>
      ) : null}

      {connectionError ? (
        <View style={[styles.banner, { backgroundColor: colors.redBg }]}>
          <Text style={{ color: colors.red, fontSize: 12, flex: 1, textAlign: 'left' }}>
            {connectionError}
          </Text>
          <Pressable
            onPress={retryConnection}
            style={[styles.retryBtn, { backgroundColor: colors.surface }]}
          >
            <Text style={{ color: colors.text, fontSize: 11.5, fontWeight: '700' }}>Retry</Text>
          </Pressable>
        </View>
      ) : null}

      {!activeHost ? (
        <View style={styles.emptyWrap}>
          <Text style={[styles.logo, { color: colors.accent }]}>π</Text>
          <Text style={[styles.emptyTitle, { color: colors.text }]}>Connect your phone to Pui</Text>
          <Text style={[styles.emptySub, { color: colors.muted }]}>
            Open Pui Desktop → Settings → Remote &amp; Mobile, enable remote access, and scan the
            Tailscale QR code.
          </Text>
          <Pressable
            onPress={() => router.push('/connect')}
            style={[styles.cta, { backgroundColor: colors.accent }]}
          >
            <Ionicons name="qr-code-outline" size={18} color="#fff" />
            <Text style={styles.ctaText}>Pair with Pui Desktop</Text>
          </Pressable>
        </View>
      ) : (
        <>
          {searchOpen ? (
            <View style={styles.searchWrap}>
              <View
                style={[
                  styles.searchBox,
                  { backgroundColor: colors.surface, borderColor: colors.border },
                ]}
              >
                <Ionicons name="search-outline" size={16} color={colors.dim} />
                <TextInput
                  autoFocus
                  value={query}
                  onChangeText={setQuery}
                  placeholder="Search sessions, projects, devices…"
                  placeholderTextColor={colors.dim}
                  style={[styles.searchInput, { color: colors.text }]}
                />
                {query ? (
                  <Pressable onPress={() => setQuery('')}>
                    <Ionicons name="close-circle" size={16} color={colors.dim} />
                  </Pressable>
                ) : null}
              </View>
            </View>
          ) : null}

          {/* SESSIONS bar with filter chips & Group/Sort button */}
          <View style={styles.sectionToolbar}>
            <View style={styles.filterGroup}>
              {(
                [
                  ['all', 'All'],
                  ['phone', 'Phone'],
                  ['running', 'Live'],
                  ['attention', 'Alerts'],
                ] as [SessionFilter, string][]
              ).map(([key, label]) => {
                const active = filter === key;
                return (
                  <Pressable
                    key={key}
                    onPress={() => setFilter(key)}
                    style={[
                      styles.filterChip,
                      {
                        backgroundColor: active ? colors.accentBg : 'transparent',
                        borderColor: active ? colors.accent : 'transparent',
                      },
                    ]}
                  >
                    <Text
                      numberOfLines={1}
                      style={{
                        color: active ? colors.accent : colors.muted,
                        fontSize: 12,
                        fontWeight: '600',
                      }}
                    >
                      {label}
                    </Text>
                  </Pressable>
                );
              })}
            </View>

            <Pressable
              onPress={() => setSortModalOpen(true)}
              style={[
                styles.sortBtn,
                { backgroundColor: colors.surface, borderColor: colors.border },
              ]}
            >
              <Ionicons name="swap-vertical-outline" size={14} color={colors.muted} />
              <Text numberOfLines={1} style={{ color: colors.muted, fontSize: 11.5, fontWeight: '600' }}>
                {grouping === 'date' ? 'Date' : grouping === 'project' ? 'Project' : 'Flat'}
              </Text>
            </Pressable>
          </View>

          <View style={styles.listContainer}>
            <SectionList
              {...bindings}
              sections={sections}
              keyExtractor={(item) => item.path}
              stickySectionHeadersEnabled={false}
              refreshControl={
                <RefreshControl refreshing={refreshing} onRefresh={() => void onRefresh()} />
              }
              contentContainerStyle={{ paddingBottom: 88, paddingTop: 2 }}
              renderSectionHeader={({ section }) =>
                section.title ? (
                  <View style={styles.dateHeader}>
                    <Text numberOfLines={1} style={[styles.dateHeaderText, { color: colors.dim }]}>
                      {section.title}
                    </Text>
                    <View style={[styles.dateHeaderLine, { backgroundColor: colors.hairline }]} />
                  </View>
                ) : null
              }
              renderItem={renderSessionRow}
              ListEmptyComponent={
                <View style={styles.emptyWrap}>
                  <Text style={[styles.emptyTitle, { color: colors.muted }]}>No sessions found</Text>
                  <Text style={[styles.emptySub, { color: colors.dim }]}>
                    Tap + New session below to start a chat from your phone.
                  </Text>
                </View>
              }
            />
            <CustomScrollbarOverlay {...metrics} />
          </View>

          {/* Clean floating + New session pill */}
          <View pointerEvents="box-none" style={styles.fabContainer}>
            <Pressable
              onPress={() => setNewOpen(true)}
              style={[styles.newPill, { backgroundColor: colors.accent }]}
            >
              <Ionicons name="add" size={19} color="#fff" />
              <Text style={styles.newPillText}>New session</Text>
            </Pressable>
          </View>
        </>
      )}

      {/* Grouping & Ordering bottom sheet (automatically persisted!) */}
      <Modal
        visible={sortModalOpen}
        transparent
        animationType="fade"
        onRequestClose={() => setSortModalOpen(false)}
      >
        <Pressable style={styles.modalBg} onPress={() => setSortModalOpen(false)}>
          <Pressable
            onPress={(e) => e.stopPropagation()}
            style={[
              styles.sheet,
              { backgroundColor: colors.surface, borderColor: colors.border },
            ]}
          >
            <Text style={[styles.sheetTitle, { color: colors.text }]}>Group sessions by</Text>
            {(
              [
                ['date', 'Date (Today, Yesterday, Last week)'],
                ['project', 'Project folder'],
                ['none', 'Flat list (no grouping)'],
              ] as [SessionGrouping, string][]
            ).map(([key, label]) => (
              <Pressable
                key={key}
                onPress={() => updatePrefs({ defaultGrouping: key })}
                style={[
                  styles.sheetRow,
                  grouping === key && { backgroundColor: colors.accentBg },
                ]}
              >
                <Text numberOfLines={1} style={{ color: colors.text, fontSize: 14, flex: 1 }}>
                  {label}
                </Text>
                {grouping === key ? (
                  <Ionicons name="checkmark" size={17} color={colors.accent} />
                ) : null}
              </Pressable>
            ))}

            <Text style={[styles.sheetTitle, { color: colors.text, marginTop: 14 }]}>Order by</Text>
            {(
              [
                ['recent', 'Recent activity'],
                ['running_first', 'Running & approvals first'],
                ['created', 'Created date'],
                ['title', 'Title (A–Z)'],
              ] as [SessionOrdering, string][]
            ).map(([key, label]) => (
              <Pressable
                key={key}
                onPress={() => updatePrefs({ defaultOrdering: key })}
                style={[
                  styles.sheetRow,
                  ordering === key && { backgroundColor: colors.accentBg },
                ]}
              >
                <Text numberOfLines={1} style={{ color: colors.text, fontSize: 14, flex: 1 }}>
                  {label}
                </Text>
                {ordering === key ? (
                  <Ionicons name="checkmark" size={17} color={colors.accent} />
                ) : null}
              </Pressable>
            ))}
          </Pressable>
        </Pressable>
      </Modal>

      {/* Long-press Session Management Sheet (Pin / Rename / Fork / Delete) */}
      <Modal
        visible={Boolean(actionTarget)}
        transparent
        animationType="fade"
        onRequestClose={() => setActionTarget(null)}
      >
        <View style={styles.modalBg}>
          <Pressable
            style={StyleSheet.absoluteFill}
            onPress={() => setActionTarget(null)}
          />
          {actionTarget ? (
            <View
              style={[
                styles.sheet,
                { backgroundColor: colors.surface, borderColor: colors.border },
              ]}
            >
              <Text numberOfLines={1} style={[styles.sheetTitle, { color: colors.text }]}>
                {actionTarget.title}
              </Text>

              {renameMode ? (
                <View style={{ gap: 10, paddingTop: 4 }}>
                  <TextInput
                    autoFocus
                    value={renameDraft}
                    onChangeText={setRenameDraft}
                    placeholder="New session title"
                    placeholderTextColor={colors.dim}
                    style={[
                      styles.renameInput,
                      {
                        backgroundColor: colors.bg,
                        color: colors.text,
                        borderColor: colors.border,
                      },
                    ]}
                  />
                  <View style={{ flexDirection: 'row', gap: 8, justifyContent: 'flex-end' }}>
                    <Pressable
                      onPress={() => setRenameMode(false)}
                      style={[styles.actionMiniBtn, { borderColor: colors.border }]}
                    >
                      <Text style={{ color: colors.muted, fontSize: 13 }}>Cancel</Text>
                    </Pressable>
                    <Pressable
                      onPress={async () => {
                        const trimmed = renameDraft.trim();
                        if (trimmed && transport) {
                          await transport.renameSession(actionTarget.path, trimmed).catch(() => {});
                          setTimeout(() => void refreshSessions().catch(() => {}), 200);
                        }
                        setActionTarget(null);
                      }}
                      style={[styles.actionMiniBtn, { backgroundColor: colors.accent, borderColor: colors.accent }]}
                    >
                      <Text style={{ color: '#fff', fontSize: 13, fontWeight: '600' }}>Save</Text>
                    </Pressable>
                  </View>
                </View>
              ) : (
                <>
                  <Pressable
                    onPress={async () => {
                      if (transport) {
                        await transport
                          .pinSession(actionTarget.path, !actionTarget.pinned)
                          .catch(() => {});
                        setTimeout(() => void refreshSessions().catch(() => {}), 200);
                      }
                      setActionTarget(null);
                    }}
                    style={[styles.sheetRow, { gap: 10 }]}
                  >
                    <Ionicons
                      name={actionTarget.pinned ? 'pin-outline' : 'pin'}
                      size={17}
                      color={colors.accent}
                    />
                    <Text style={{ color: colors.text, fontSize: 14, flex: 1 }}>
                      {actionTarget.pinned ? 'Unpin session' : 'Pin session'}
                    </Text>
                  </Pressable>

                  <Pressable
                    onPress={() => setRenameMode(true)}
                    style={[styles.sheetRow, { gap: 10 }]}
                  >
                    <Ionicons name="pencil-outline" size={17} color={colors.muted} />
                    <Text style={{ color: colors.text, fontSize: 14, flex: 1 }}>
                      Rename session
                    </Text>
                  </Pressable>

                  <Pressable
                    onPress={async () => {
                      const target = actionTarget;
                      setActionTarget(null);
                      if (!transport) return;
                      try {
                        const res = await transport.forkSession(target.path, clientOrigin());
                        void refreshSessions().catch(() => {});
                        if (res.path) {
                          void transport.openSession(res.path).catch(() => {});
                          router.push({
                            pathname: '/chat/[id]',
                            params: { id: encodeURIComponent(res.path) },
                          });
                        }
                      } catch {
                        // ignore
                      }
                    }}
                    style={[styles.sheetRow, { gap: 10 }]}
                  >
                    <Ionicons name="git-branch-outline" size={17} color={colors.muted} />
                    <Text style={{ color: colors.text, fontSize: 14, flex: 1 }}>
                      Fork conversation
                    </Text>
                  </Pressable>

                  <Pressable
                    onPress={async () => {
                      if (!confirmDelete) {
                        setConfirmDelete(true);
                        return;
                      }
                      const target = actionTarget;
                      setActionTarget(null);
                      if (transport) {
                        await transport.deleteSession(target.path).catch(() => {});
                        setTimeout(() => void refreshSessions().catch(() => {}), 200);
                      }
                    }}
                    style={[styles.sheetRow, { gap: 10 }]}
                  >
                    <Ionicons name="trash-outline" size={17} color={colors.red} />
                    <Text style={{ color: colors.red, fontSize: 14, fontWeight: confirmDelete ? '700' : '500', flex: 1 }}>
                      {confirmDelete ? 'Tap again to confirm delete' : 'Delete session'}
                    </Text>
                  </Pressable>
                </>
              )}
            </View>
          ) : null}
        </View>
      </Modal>

      <UsageSheet
        visible={usageOpen}
        transport={transport}
        onClose={() => setUsageOpen(false)}
      />

      <NewSessionModal
        visible={newOpen}
        projects={hostInfo?.projects ?? []}
        permissionModes={defaultPermissionModes}
        deviceLabel={`${clientOrigin().platform === 'ios' ? 'iOS' : 'Android'} · ${deviceName}`}
        onClose={() => setNewOpen(false)}
        onCreate={async ({ cwd, permissionMode, prompt }) => {
          if (!transport) return;
          await transport.createSession({
            cwd,
            permission_mode: permissionMode,
            prompt,
            origin: clientOrigin(),
          });
          setTimeout(() => {
            void refreshSessions();
          }, 500);
        }}
      />
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  safe: {
    flex: 1,
    direction: 'ltr',
  },
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 14,
    paddingVertical: 10,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  hostChip: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 7,
    paddingHorizontal: 12,
    paddingVertical: 7,
    borderRadius: 999,
    borderWidth: 1,
    maxWidth: '68%',
  },
  dot: {
    width: 8,
    height: 8,
    borderRadius: 4,
    flexShrink: 0,
  },
  hostTitle: {
    fontSize: 14,
    fontWeight: '700',
    flexShrink: 1,
  },
  headerActions: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 4,
  },
  headerIconBtn: {
    width: 36,
    height: 36,
    borderRadius: 10,
    alignItems: 'center',
    justifyContent: 'center',
  },
  banner: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    paddingHorizontal: 14,
    paddingVertical: 8,
  },
  retryBtn: {
    paddingHorizontal: 10,
    paddingVertical: 4,
    borderRadius: 6,
  },
  searchWrap: {
    paddingHorizontal: 14,
    paddingTop: 10,
  },
  searchBox: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    borderWidth: 1,
    borderRadius: 11,
    paddingHorizontal: 11,
    height: 40,
  },
  searchInput: {
    flex: 1,
    fontSize: 13.5,
    textAlign: 'left',
  },
  sectionToolbar: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 14,
    paddingTop: 10,
    paddingBottom: 6,
    gap: 8,
  },
  filterGroup: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 4,
    flexShrink: 1,
  },
  filterChip: {
    paddingHorizontal: 9,
    paddingVertical: 5,
    borderRadius: 999,
    borderWidth: StyleSheet.hairlineWidth,
  },
  sortBtn: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 5,
    paddingHorizontal: 9,
    paddingVertical: 5,
    borderRadius: 8,
    borderWidth: StyleSheet.hairlineWidth,
    flexShrink: 0,
  },
  listContainer: {
    flex: 1,
    position: 'relative',
  },
  dateHeader: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    paddingHorizontal: 16,
    paddingTop: 14,
    paddingBottom: 6,
  },
  dateHeaderText: {
    fontSize: 11.5,
    fontWeight: '600',
  },
  dateHeaderLine: {
    flex: 1,
    height: StyleSheet.hairlineWidth,
  },
  sessionRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 11,
    marginHorizontal: 8,
    paddingHorizontal: 10,
    paddingVertical: 11,
    borderRadius: 10,
  },
  rowDot: {
    width: 7,
    height: 7,
    borderRadius: 3.5,
    flexShrink: 0,
  },
  rowMiddle: {
    flex: 1,
    minWidth: 0,
    gap: 3,
  },
  rowTitle: {
    fontSize: 14,
    fontWeight: '500',
  },
  rowSubLine: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  rowMeta: {
    fontSize: 11.5,
    flexShrink: 1,
  },
  originPill: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 3,
    paddingHorizontal: 6,
    paddingVertical: 1.5,
    borderRadius: 999,
    flexShrink: 0,
  },
  originPillText: {
    fontSize: 10,
    fontWeight: '600',
  },
  rowAge: {
    fontSize: 11.5,
    flexShrink: 0,
  },
  fabContainer: {
    position: 'absolute',
    left: 16,
    right: 16,
    bottom: 18,
    flexDirection: 'row',
    justifyContent: 'flex-start',
  },
  newPill: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    paddingHorizontal: 18,
    paddingVertical: 12,
    borderRadius: 999,
    elevation: 4,
    shadowColor: '#000',
    shadowOpacity: 0.3,
    shadowRadius: 10,
    shadowOffset: { width: 0, height: 4 },
  },
  newPillText: {
    color: '#fff',
    fontSize: 14,
    fontWeight: '700',
  },
  modalBg: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.58)',
    justifyContent: 'flex-end',
    padding: 12,
  },
  sheet: {
    borderWidth: 1,
    borderRadius: 18,
    padding: 14,
    direction: 'ltr',
  },
  sheetTitle: {
    fontSize: 13,
    fontWeight: '700',
    marginBottom: 6,
    marginLeft: 4,
  },
  sheetRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 10,
    paddingVertical: 11,
    borderRadius: 10,
  },
  emptyWrap: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
    padding: 32,
    gap: 10,
  },
  logo: {
    fontSize: 52,
    fontWeight: '700',
  },
  emptyTitle: {
    fontSize: 18,
    fontWeight: '700',
    textAlign: 'center',
  },
  emptySub: {
    fontSize: 13.5,
    lineHeight: 20,
    textAlign: 'center',
  },
  cta: {
    marginTop: 8,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingHorizontal: 18,
    paddingVertical: 12,
    borderRadius: 10,
  },
  ctaText: {
    color: '#fff',
    fontSize: 14,
    fontWeight: '700',
  },
  renameInput: {
    borderWidth: 1,
    borderRadius: 10,
    paddingHorizontal: 12,
    paddingVertical: 9,
    fontSize: 14,
    textAlign: 'left',
  },
  actionMiniBtn: {
    borderWidth: 1,
    borderRadius: 9,
    paddingHorizontal: 14,
    paddingVertical: 7,
  },
});
