import React, { useEffect, useMemo, useState } from 'react';
import {
  ActivityIndicator,
  Keyboard,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { router } from 'expo-router';
import { Ionicons } from '@expo/vector-icons';
import { Composer } from '../src/components/Composer';
import { ImageModal } from '../src/components/ImageModal';
import type { ModelItem, PermissionModeOption } from '../src/protocol/types';
import { usePui } from '../src/store/connectionStore';
import { usePalette } from '../src/theme/colors';

const DEFAULT_PERMISSION_MODES: PermissionModeOption[] = [
  { key: 'auto', label: 'Auto' },
  { key: 'manual', label: 'Manual' },
  { key: 'acceptEdits', label: 'Accept edits' },
  { key: 'plan', label: 'Plan' },
  { key: 'dontAsk', label: "Don't ask" },
  { key: 'bypassPermissions', label: 'Bypass' },
];

const DEFAULT_LEVELS = ['off', 'low', 'medium', 'high'];

const STARTERS = [
  {
    icon: 'search-outline' as const,
    title: 'Understand this project',
    prompt: "Explore this project and explain its architecture. Don't change any files.",
  },
  {
    icon: 'terminal-outline' as const,
    title: 'Find the next step',
    prompt: "Inspect this project and suggest three useful next steps. Don't change any files.",
  },
  {
    icon: 'sparkles-outline' as const,
    title: 'Build something new',
    prompt: "I'd like to build a new feature. First help me understand where to start.",
  },
];

function looksLikeGitRepo(val: string): boolean {
  const s = val.trim();
  return /^(git@|https:\/\/|ssh:\/\/)/.test(s) || /^[a-zA-Z0-9._-]+\/[a-zA-Z0-9._-]+$/.test(s);
}

export default function NewChatScreen() {
  const colors = usePalette();
  const {
    hosts,
    activeHost,
    selectHost,
    connectionState,
    hostInfo,
    transport,
    detailsByPath,
    clientOrigin,
    refreshSessions,
  } = usePui();

  const projectChoices = useMemo(() => {
    const rawDirs = hostInfo?.projects ?? [];
    const catalog = hostInfo?.catalog_projects ?? [];
    const entries: {
      key: string;
      label: string;
      cwd: string;
      gitRemote?: string;
      cloned: boolean;
    }[] = [];
    const seen = new Set<string>();

    // First include all synced catalog projects (all Mac repos available on cloud!).
    for (const c of catalog) {
      const k = c.name.toLowerCase();
      if (seen.has(k)) continue;
      seen.add(k);
      entries.push({
        key: `cat:${c.name}`,
        label: c.name,
        cwd: c.cloned && c.path ? c.path : c.name,
        gitRemote: c.cloned ? undefined : c.git_remote,
        cloned: c.cloned,
      });
    }

    // Then include any local/cloned workspace folders not already in catalog.
    for (const p of rawDirs) {
      const short = p.split('/').filter(Boolean).pop() ?? p;
      // Hide the bare `workspaces` root folder when real projects exist.
      if (short === 'workspaces' && (entries.length > 0 || rawDirs.length > 1)) continue;
      const k = short.toLowerCase();
      if (seen.has(k)) continue;
      seen.add(k);
      entries.push({
        key: `dir:${p}`,
        label: short,
        cwd: p,
        cloned: true,
      });
    }
    return entries;
  }, [hostInfo?.projects, hostInfo?.catalog_projects]);

  const permissionModes = useMemo(
    () =>
      hostInfo?.permission_modes?.length ? hostInfo.permission_modes : DEFAULT_PERMISSION_MODES,
    [hostInfo?.permission_modes],
  );

  const [selectedKey, setSelectedKey] = useState(projectChoices[0]?.key ?? '');
  const [customRepoOrPath, setCustomRepoOrPath] = useState('');
  const [permissionMode, setPermissionMode] = useState(permissionModes[0]?.key ?? 'auto');
  const [models, setModels] = useState<ModelItem[]>([]);
  const [modelId, setModelId] = useState('');
  const [provider, setProvider] = useState('');
  const [thinking, setThinking] = useState('medium');
  const [levels, setLevels] = useState<string[]>(DEFAULT_LEVELS);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [previewImage, setPreviewImage] = useState<{ uri: string; title?: string } | null>(null);
  const [composerPrefill, setComposerPrefill] = useState<{ id: number; text?: string } | null>(
    null,
  );
  const [androidKeyboardHeight, setAndroidKeyboardHeight] = useState(0);

  useEffect(() => {
    if (Platform.OS !== 'android') return;
    const showSub = Keyboard.addListener('keyboardDidShow', (e) => {
      setAndroidKeyboardHeight(e.endCoordinates?.height ?? 0);
    });
    const hideSub = Keyboard.addListener('keyboardDidHide', () => {
      setAndroidKeyboardHeight(0);
    });
    return () => {
      showSub.remove();
      hideSub.remove();
    };
  }, []);

  // Keep selected project synced whenever the user switches host.
  useEffect(() => {
    if (projectChoices.length > 0 && !projectChoices.some((p) => p.key === selectedKey)) {
      setSelectedKey(projectChoices[0].key);
    } else if (projectChoices.length === 0) {
      setSelectedKey('');
    }
  }, [projectChoices, selectedKey]);

  // Load models/effort options for the active host (fallback to any cached session detail).
  useEffect(() => {
    let cancelled = false;
    const seedFromCache = () => {
      const cached = Object.values(detailsByPath).find((d) => d.models?.length > 0);
      if (cached) {
        setModels(cached.models);
        if (!modelId && cached.model) setModelId(cached.model);
        if (!provider && cached.provider) setProvider(cached.provider);
        if (cached.thinking) setThinking(cached.thinking);
        if (cached.levels?.length) setLevels(cached.levels);
      }
    };
    if (!transport) {
      seedFromCache();
      return;
    }
    void transport
      .getModels()
      .then((res) => {
        if (cancelled) return;
        if (res.models?.length) {
          setModels(res.models);
          if (res.current_model) setModelId(res.current_model);
          if (res.current_provider) setProvider(res.current_provider);
          if (res.current_thinking) setThinking(res.current_thinking);
          if (res.levels?.length) setLevels(res.levels);
        } else {
          seedFromCache();
        }
      })
      .catch(() => {
        if (!cancelled) seedFromCache();
      });
    return () => {
      cancelled = true;
    };
  }, [transport, activeHost?.id]);

  const selectedChoice = useMemo(
    () => projectChoices.find((p) => p.key === selectedKey) ?? projectChoices[0] ?? null,
    [projectChoices, selectedKey],
  );
  const effectiveCwd = customRepoOrPath.trim() || selectedChoice?.cwd || '';
  const effectiveGitRemote = customRepoOrPath.trim()
    ? looksLikeGitRepo(customRepoOrPath.trim())
      ? customRepoOrPath.trim()
      : undefined
    : selectedChoice?.gitRemote;
  const isCloudHost = Boolean(
    activeHost?.name.toLowerCase().includes('cloud') ||
      hostInfo?.pui_version?.includes('cloud'),
  );
  // Cloud agents always run in Auto mode inside an isolated pui_agent/… worktree.
  useEffect(() => {
    if (isCloudHost && permissionMode !== 'auto') {
      setPermissionMode('auto');
    }
  }, [isCloudHost, permissionMode]);
  const currentModelLabel = useMemo(() => {
    if (!modelId) return 'Default model';
    const match = models.find((m) => m.id === modelId);
    return match?.name || modelId;
  }, [modelId, models]);

  const headerSubtitle = useMemo(() => {
    const hostLabel = activeHost?.name ?? 'Host';
    const projShort = customRepoOrPath.trim()
      ? looksLikeGitRepo(customRepoOrPath.trim())
        ? customRepoOrPath.trim()
        : customRepoOrPath.trim().split('/').filter(Boolean).pop() ?? customRepoOrPath.trim()
      : selectedChoice?.label ?? 'Choose project';
    return `${hostLabel} · ${projShort}`;
  }, [activeHost?.name, customRepoOrPath, selectedChoice?.label]);

  const handleSend = async (
    text: string,
    images: { data_base64: string; mime: string; name?: string }[],
  ) => {
    if (!transport || creating) return;
    if (!effectiveCwd) {
      setCreateError('Pick a project folder or enter a Git repo (e.g. owner/repo) first.');
      return;
    }
    setCreating(true);
    setCreateError(null);
    try {
      const origin = clientOrigin();
      const res = await transport.createSession({
        cwd: effectiveCwd,
        git_remote: effectiveGitRemote,
        permission_mode: isCloudHost ? 'auto' : permissionMode,
        model: modelId || undefined,
        provider: provider || undefined,
        thinking: thinking || undefined,
        prompt: text.trim() || undefined,
        images: images.length > 0 ? images : undefined,
        origin,
      });

      if (res.path) {
        void refreshSessions();
        router.replace(`/chat/${encodeURIComponent(res.path)}`);
        return;
      }

      // Desktop creates parked background sessions asynchronously; poll briefly for its new path.
      for (let attempt = 0; attempt < 6; attempt++) {
        await new Promise((r) => setTimeout(r, 350));
        try {
          const list = await transport.listSessions();
          const newest = list.sessions.find(
            (s) =>
              s.origin?.created_at === origin.created_at ||
              (s.cwd === effectiveCwd && s.busy),
          );
          if (newest?.path) {
            void refreshSessions();
            router.replace(`/chat/${encodeURIComponent(newest.path)}`);
            return;
          }
        } catch {
          // retry
        }
      }
      void refreshSessions();
      router.back();
    } catch (err) {
      setCreateError(err instanceof Error ? err.message : 'Could not start session.');
    } finally {
      setCreating(false);
    }
  };

  return (
    <SafeAreaView style={[styles.safe, { backgroundColor: colors.bg }]} edges={['top', 'bottom']}>
      {/* Regular Chat Header */}
      <View style={[styles.header, { backgroundColor: colors.surface, borderColor: colors.border }]}>
        <Pressable onPress={() => router.back()} style={styles.headerBtn} hitSlop={8}>
          <Ionicons name="chevron-back" size={22} color={colors.text} />
        </Pressable>
        <View style={styles.headerCenter}>
          <View style={styles.titleRow}>
            <Ionicons
              name={isCloudHost ? 'cloud-outline' : 'laptop-outline'}
              size={14}
              color={isCloudHost ? colors.green : colors.accent}
            />
            <Text numberOfLines={1} style={[styles.headerTitle, { color: colors.text }]}>
              New chat
            </Text>
          </View>
          <Text numberOfLines={1} style={[styles.headerSub, { color: colors.muted }]}>
            {headerSubtitle}
          </Text>
        </View>
        <View
          style={[
            styles.statusDot,
            {
              backgroundColor:
                connectionState === 'connected'
                  ? colors.green
                  : connectionState === 'connecting'
                    ? colors.amber
                    : colors.red,
            },
          ]}
        />
      </View>

      <KeyboardAvoidingView
        style={{ flex: 1 }}
        behavior={Platform.OS === 'ios' ? 'padding' : undefined}
        keyboardVerticalOffset={0}
      >
        <ScrollView
          style={{ flex: 1 }}
          contentContainerStyle={styles.canvas}
          keyboardShouldPersistTaps="handled"
        >
          {/* Welcome emblem */}
          <View style={styles.hero}>
            <View style={[styles.emblem, { backgroundColor: colors.accentBg }]}>
              <Text style={[styles.emblemText, { color: colors.accent }]}>π</Text>
            </View>
            <Text style={[styles.heroTitle, { color: colors.text }]}>Start a new chat</Text>
            <Text style={[styles.heroSub, { color: colors.muted }]}>
              Choose environment and project, then pick model, effort &amp; permissions in the
              composer below.
            </Text>
          </View>

          {/* Environment (Where to run) */}
          {hosts.length > 0 ? (
            <View style={styles.section}>
              <Text style={[styles.sectionLabel, { color: colors.muted }]}>ENVIRONMENT</Text>
              <View style={styles.pillRow}>
                {hosts.map((h) => {
                  const active = h.id === activeHost?.id;
                  const cloud = h.name.toLowerCase().includes('cloud');
                  return (
                    <Pressable
                      key={h.id}
                      onPress={() => {
                        setCreateError(null);
                        void selectHost(h.id);
                      }}
                      style={[
                        styles.envPill,
                        {
                          backgroundColor: active ? colors.accentBg : colors.surface,
                          borderColor: active ? colors.accent : colors.border,
                        },
                      ]}
                    >
                      <Ionicons
                        name={cloud ? 'cloud-outline' : 'laptop-outline'}
                        size={15}
                        color={active ? (cloud ? colors.green : colors.accent) : colors.muted}
                      />
                      <Text
                        numberOfLines={1}
                        style={{
                          color: active ? colors.text : colors.muted,
                          fontSize: 13,
                          fontWeight: '600',
                        }}
                      >
                        {h.name}
                      </Text>
                      {cloud ? (
                        <Text style={[styles.badge24, { color: colors.green }]}>24/7</Text>
                      ) : null}
                    </Pressable>
                  );
                })}
              </View>
            </View>
          ) : null}

          {/* Project / Repository */}
          <View style={styles.section}>
            <Text style={[styles.sectionLabel, { color: colors.muted }]}>
              {isCloudHost ? 'PROJECT OR GIT REPO (AUTO-CLONES)' : 'PROJECT FOLDER'}
            </Text>
            {projectChoices.length > 0 ? (
              <View style={styles.projectGrid}>
                {projectChoices.map((p) => {
                  const active = selectedChoice?.key === p.key && !customRepoOrPath.trim();
                  return (
                    <Pressable
                      key={p.key}
                      onPress={() => {
                        setSelectedKey(p.key);
                        setCustomRepoOrPath('');
                        setCreateError(null);
                      }}
                      style={[
                        styles.projectChip,
                        {
                          backgroundColor: active ? colors.accentBg : colors.surface,
                          borderColor: active ? colors.accent : colors.border,
                        },
                      ]}
                    >
                      <Ionicons
                        name={p.cloned ? 'folder-outline' : 'cloud-download-outline'}
                        size={14}
                        color={active ? colors.accent : colors.muted}
                      />
                      <Text
                        numberOfLines={1}
                        style={{
                          color: active ? colors.text : colors.muted,
                          fontSize: 13,
                          fontWeight: active ? '600' : '500',
                          flexShrink: 1,
                        }}
                      >
                        {p.label}
                      </Text>
                    </Pressable>
                  );
                })}
              </View>
            ) : null}

            <View
              style={[
                styles.repoInputBox,
                { backgroundColor: colors.surface, borderColor: colors.border },
              ]}
            >
              <Ionicons
                name={isCloudHost ? 'git-branch-outline' : 'folder-open-outline'}
                size={15}
                color={colors.dim}
              />
              <TextInput
                value={customRepoOrPath}
                onChangeText={(v) => {
                  setCustomRepoOrPath(v);
                  setCreateError(null);
                }}
                placeholder={
                  isCloudHost
                    ? 'Auto-clone any repo (e.g. owner/repo or git@github.com:…)'
                    : 'Or enter another folder path on this host…'
                }
                placeholderTextColor={colors.dim}
                autoCapitalize="none"
                autoCorrect={false}
                style={[styles.repoInput, { color: colors.text }]}
              />
              {customRepoOrPath ? (
                <Pressable onPress={() => setCustomRepoOrPath('')}>
                  <Ionicons name="close-circle" size={16} color={colors.dim} />
                </Pressable>
              ) : null}
            </View>
          </View>

          {/* Quick starter prompts */}
          <View style={styles.section}>
            <Text style={[styles.sectionLabel, { color: colors.muted }]}>QUICK STARTERS</Text>
            <View style={{ gap: 8 }}>
              {STARTERS.map((s) => (
                <Pressable
                  key={s.title}
                  onPress={() => setComposerPrefill({ id: Date.now(), text: s.prompt })}
                  style={[
                    styles.starterCard,
                    { backgroundColor: colors.surface, borderColor: colors.border },
                  ]}
                >
                  <Ionicons name={s.icon} size={15} color={colors.accent} />
                  <Text style={[styles.starterTitle, { color: colors.text }]}>{s.title}</Text>
                  <Ionicons name="arrow-up-outline" size={14} color={colors.dim} />
                </Pressable>
              ))}
            </View>
          </View>

          {creating ? (
            <View style={styles.creatingRow}>
              <ActivityIndicator size="small" color={colors.accent} />
              <Text style={{ color: colors.muted, fontSize: 12.5 }}>
                {effectiveGitRemote
                  ? `Cloning ${selectedChoice?.label || customRepoOrPath.trim()} on ${activeHost?.name ?? 'cloud'}…`
                  : `Starting session on ${activeHost?.name ?? 'host'}…`}
              </Text>
            </View>
          ) : null}

          {createError ? (
            <View
              style={[
                styles.errorBox,
                { backgroundColor: colors.redBg, borderColor: colors.red },
              ]}
            >
              <Text style={{ color: colors.red, fontSize: 12.5 }}>{createError}</Text>
            </View>
          ) : null}
        </ScrollView>

        {/* Full Native Chat Composer pinned to bottom */}
        <View
          style={{
            paddingBottom:
              Platform.OS === 'android' && androidKeyboardHeight > 0
                ? androidKeyboardHeight + 6
                : 0,
          }}
        >
          <Composer
            cwd={effectiveCwd}
            busy={creating}
            prefill={composerPrefill}
            permissionMode={permissionMode}
            permissionModes={permissionModes}
            currentModelId={modelId}
            currentModelLabel={currentModelLabel}
            currentThinking={thinking}
            thinkingLevels={levels}
            models={models}
            slashCommands={[]}
            onFetchFileMentions={async (cwd, query) => {
              if (!transport || !cwd || looksLikeGitRepo(cwd)) return [];
              try {
                return await transport.getFileMentions(cwd, query);
              } catch {
                return [];
              }
            }}
            onSend={handleSend}
            onAbort={async () => {}}
            onChangePermission={async (key) => {
              setPermissionMode(isCloudHost ? 'auto' : key);
            }}
            onChangeModel={async (prov, id) => {
              setProvider(prov);
              setModelId(id);
            }}
            onChangeThinking={async (lvl) => {
              setThinking(lvl);
            }}
            onPreviewImage={(uri, title) => setPreviewImage({ uri, title })}
          />
        </View>
      </KeyboardAvoidingView>

      <ImageModal
        uri={previewImage?.uri ?? null}
        title={previewImage?.title}
        onClose={() => setPreviewImage(null)}
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
    paddingHorizontal: 10,
    paddingVertical: 9,
    borderBottomWidth: StyleSheet.hairlineWidth,
    gap: 8,
  },
  headerBtn: {
    width: 34,
    height: 34,
    borderRadius: 9,
    alignItems: 'center',
    justifyContent: 'center',
  },
  headerCenter: {
    flex: 1,
    minWidth: 0,
  },
  titleRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  headerTitle: {
    fontSize: 15,
    fontWeight: '700',
  },
  headerSub: {
    fontSize: 11.5,
    marginTop: 1,
  },
  statusDot: {
    width: 8,
    height: 8,
    borderRadius: 4,
    marginRight: 8,
  },
  canvas: {
    paddingHorizontal: 16,
    paddingTop: 18,
    paddingBottom: 20,
    gap: 18,
  },
  hero: {
    alignItems: 'center',
    gap: 6,
    paddingVertical: 6,
  },
  emblem: {
    width: 48,
    height: 48,
    borderRadius: 14,
    alignItems: 'center',
    justifyContent: 'center',
    marginBottom: 4,
  },
  emblemText: {
    fontSize: 30,
    fontWeight: '700',
  },
  heroTitle: {
    fontSize: 19,
    fontWeight: '700',
  },
  heroSub: {
    fontSize: 12.5,
    textAlign: 'center',
    lineHeight: 18,
    maxWidth: 320,
  },
  section: {
    gap: 8,
  },
  sectionLabel: {
    fontSize: 11,
    fontWeight: '700',
    letterSpacing: 0.6,
  },
  pillRow: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    gap: 8,
  },
  envPill: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 7,
    paddingHorizontal: 13,
    paddingVertical: 9,
    borderRadius: 11,
    borderWidth: 1,
  },
  badge24: {
    fontSize: 10.5,
    fontWeight: '700',
  },
  projectGrid: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    gap: 8,
  },
  projectChip: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    paddingHorizontal: 11,
    paddingVertical: 8,
    borderRadius: 10,
    borderWidth: 1,
    maxWidth: '100%',
  },
  repoInputBox: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    borderWidth: 1,
    borderRadius: 10,
    paddingHorizontal: 11,
    height: 42,
  },
  repoInput: {
    flex: 1,
    fontSize: 13,
    paddingVertical: 0,
  },
  starterCard: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    paddingHorizontal: 13,
    paddingVertical: 11,
    borderRadius: 11,
    borderWidth: 1,
  },
  starterTitle: {
    flex: 1,
    fontSize: 13,
    fontWeight: '500',
  },
  creatingRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 8,
    paddingVertical: 6,
  },
  errorBox: {
    borderWidth: 1,
    borderRadius: 10,
    paddingHorizontal: 12,
    paddingVertical: 9,
  },
});
