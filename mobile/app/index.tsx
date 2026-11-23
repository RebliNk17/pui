import React, { useMemo, useState } from 'react';
import {
  FlatList,
  Pressable,
  RefreshControl,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { router } from 'expo-router';
import { Ionicons } from '@expo/vector-icons';
import { NewSessionModal } from '../src/components/NewSessionModal';
import { OriginBadge } from '../src/components/OriginBadge';
import type { SessionSummary } from '../src/protocol/types';
import { usePui } from '../src/store/connectionStore';
import { usePalette } from '../src/theme/colors';

type Filter = 'all' | 'phone' | 'running' | 'attention';

export default function SessionsScreen() {
  const colors = usePalette();
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
  } = usePui();

  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState<Filter>('all');
  const [refreshing, setRefreshing] = useState(false);
  const [newOpen, setNewOpen] = useState(false);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return sessions.filter((s) => {
      if (filter === 'phone' && s.origin.platform !== 'android' && s.origin.platform !== 'ios') {
        return false;
      }
      if (filter === 'running' && !s.busy) return false;
      if (filter === 'attention' && !s.has_pending_dialog && !s.unread) return false;
      if (!q) return true;
      return (
        s.title.toLowerCase().includes(q) ||
        s.project_name.toLowerCase().includes(q) ||
        (s.origin.device ?? '').toLowerCase().includes(q)
      );
    });
  }, [sessions, query, filter]);

  const onRefresh = async () => {
    setRefreshing(true);
    try {
      await refreshSessions();
    } finally {
      setRefreshing(false);
    }
  };

  const openSession = (session: SessionSummary) => {
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

  return (
    <SafeAreaView style={[styles.safe, { backgroundColor: colors.bg }]}>
      <View style={[styles.header, { borderBottomColor: colors.border }]}>
        <Pressable onPress={() => router.push('/connect')} style={styles.hostButton}>
          <View style={[styles.dot, { backgroundColor: statusDot }]} />
          <View style={{ flex: 1 }}>
            <Text numberOfLines={1} style={[styles.hostTitle, { color: colors.text }]}>
              {activeHost ? activeHost.name : 'Connect to Pui'}
            </Text>
            <Text numberOfLines={1} style={[styles.hostSub, { color: colors.muted }]}>
              {activeHost
                ? `${activeHost.host}:${activeHost.port} · ${connectionState}`
                : 'Pair via QR code or Tailscale IP'}
            </Text>
          </View>
          <Ionicons name="swap-horizontal-outline" size={18} color={colors.muted} />
        </Pressable>
        {activeHost ? (
          <Pressable
            onPress={() => setNewOpen(true)}
            style={[styles.newBtn, { backgroundColor: colors.accent }]}
          >
            <Ionicons name="add" size={20} color="#fff" />
          </Pressable>
        ) : null}
      </View>

      {connectionError ? (
        <View style={[styles.banner, { backgroundColor: colors.redBg }]}>
          <Text style={{ color: colors.red, fontSize: 12, flex: 1 }}>{connectionError}</Text>
          <Pressable
            onPress={retryConnection}
            style={{
              paddingHorizontal: 10,
              paddingVertical: 4,
              borderRadius: 6,
              backgroundColor: colors.surface,
            }}
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
            Open Pui Desktop → Settings → Remote & Mobile, turn on remote access, and scan the
            Tailscale pairing QR code.
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
          <View style={styles.controls}>
            <TextInput
              value={query}
              onChangeText={setQuery}
              placeholder="Search sessions, projects, devices…"
              placeholderTextColor={colors.dim}
              style={[
                styles.search,
                { backgroundColor: colors.surface, color: colors.text, borderColor: colors.border },
              ]}
            />
            <View style={styles.filters}>
              {(
                [
                  ['all', 'All'],
                  ['phone', 'From phone'],
                  ['running', 'Running'],
                  ['attention', 'Needs you'],
                ] as [Filter, string][]
              ).map(([key, label]) => {
                const active = filter === key;
                return (
                  <Pressable
                    key={key}
                    onPress={() => setFilter(key)}
                    style={[
                      styles.filterPill,
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
                      {label}
                    </Text>
                  </Pressable>
                );
              })}
            </View>
          </View>

          <FlatList
            data={filtered}
            keyExtractor={(item) => item.path}
            refreshControl={
              <RefreshControl refreshing={refreshing} onRefresh={() => void onRefresh()} />
            }
            contentContainerStyle={{ paddingBottom: 30 }}
            renderItem={({ item }) => (
              <Pressable
                onPress={() => openSession(item)}
                style={[
                  styles.sessionCard,
                  { backgroundColor: colors.surface, borderColor: colors.border },
                ]}
              >
                <View style={styles.sessionTop}>
                  <View
                    style={[
                      styles.statusCircle,
                      {
                        backgroundColor: item.has_pending_dialog
                          ? colors.amber
                          : item.busy
                            ? colors.accent
                            : colors.green,
                      },
                    ]}
                  />
                  <Text numberOfLines={1} style={[styles.sessionTitle, { color: colors.text }]}>
                    {item.title}
                  </Text>
                  {item.has_pending_dialog ? (
                    <View style={[styles.alertBadge, { backgroundColor: colors.amberBg }]}>
                      <Text style={{ color: colors.amber, fontSize: 10.5, fontWeight: '700' }}>
                        Approval
                      </Text>
                    </View>
                  ) : null}
                </View>

                <View style={styles.sessionMeta}>
                  <OriginBadge origin={item.origin} />
                  <Text numberOfLines={1} style={[styles.projectText, { color: colors.dim }]}>
                    {item.project_name} · {item.status}
                  </Text>
                </View>
              </Pressable>
            )}
            ListEmptyComponent={
              <View style={styles.emptyWrap}>
                <Text style={[styles.emptyTitle, { color: colors.muted }]}>No sessions found</Text>
                <Text style={[styles.emptySub, { color: colors.dim }]}>
                  Tap + in the top right to start a new session from your phone.
                </Text>
              </View>
            }
          />
        </>
      )}

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
  safe: { flex: 1 },
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    paddingHorizontal: 14,
    paddingVertical: 10,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  hostButton: {
    flex: 1,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
  },
  dot: {
    width: 10,
    height: 10,
    borderRadius: 5,
  },
  hostTitle: {
    fontSize: 16,
    fontWeight: '700',
  },
  hostSub: {
    fontSize: 11.5,
  },
  newBtn: {
    width: 36,
    height: 36,
    borderRadius: 10,
    justifyContent: 'center',
    alignItems: 'center',
  },
  banner: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    paddingHorizontal: 14,
    paddingVertical: 8,
  },
  controls: {
    paddingHorizontal: 12,
    paddingVertical: 10,
    gap: 8,
  },
  search: {
    borderWidth: 1,
    borderRadius: 10,
    paddingHorizontal: 12,
    paddingVertical: 9,
    fontSize: 13.5,
  },
  filters: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    gap: 6,
  },
  filterPill: {
    paddingHorizontal: 10,
    paddingVertical: 5,
    borderRadius: 999,
    borderWidth: 1,
  },
  sessionCard: {
    marginHorizontal: 12,
    marginVertical: 4,
    padding: 12,
    borderRadius: 12,
    borderWidth: 1,
    gap: 8,
  },
  sessionTop: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  statusCircle: {
    width: 8,
    height: 8,
    borderRadius: 4,
  },
  sessionTitle: {
    flex: 1,
    fontSize: 14.5,
    fontWeight: '600',
  },
  alertBadge: {
    paddingHorizontal: 7,
    paddingVertical: 2,
    borderRadius: 999,
  },
  sessionMeta: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  projectText: {
    flex: 1,
    fontSize: 11.5,
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
});
