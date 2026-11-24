import React, { useEffect, useMemo, useState } from 'react';
import {
  ActivityIndicator,
  Keyboard,
  KeyboardAvoidingView,
  Modal,
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
import type { CloudRepoItem, ModelItem, PermissionModeOption } from '../src/protocol/types';
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

interface DropdownItem {
  key: string;
  title: string;
  subtitle: string;
  cwd: string;
  gitRemote?: string;
  cloned: boolean;
  isPrivate?: boolean;
}

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

  const isCloudHost = Boolean(
    activeHost?.name.toLowerCase().includes('cloud') ||
      hostInfo?.pui_version?.includes('cloud'),
  );

  const permissionModes = useMemo(
    () =>
      hostInfo?.permission_modes?.length ? hostInfo.permission_modes : DEFAULT_PERMISSION_MODES,
    [hostInfo?.permission_modes],
  );

  const [cloudRepos, setCloudRepos] = useState<CloudRepoItem[]>([]);
  const [reposLoading, setReposLoading] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerQuery, setPickerQuery] = useState('');
  const [selectedItem, setSelectedItem] = useState<DropdownItem | null>(null);

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

  // Cloud agents always run in Auto mode inside an isolated pui_agent/… worktree.
  useEffect(() => {
    if (isCloudHost && permissionMode !== 'auto') {
      setPermissionMode('auto');
    }
  }, [isCloudHost, permissionMode]);

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

  // Fetch GitHub org/repo list from cloud host whenever targeting a cloud host.
  const loadCloudRepos = (refresh = false) => {
    if (!transport || !isCloudHost) {
      setCloudRepos([]);
      return;
    }
    setReposLoading(true);
    void transport
      .listRepos(undefined, refresh)
      .then((res) => {
        setCloudRepos(res.repos ?? []);
      })
      .catch(() => {
        setCloudRepos([]);
      })
      .finally(() => setReposLoading(false));
  };

  useEffect(() => {
    setSelectedItem(null);
    setPickerQuery('');
    if (isCloudHost) {
      loadCloudRepos(false);
    } else {
      setCloudRepos([]);
    }
  }, [activeHost?.id, isCloudHost, transport]);

  // Unified searchable dropdown items — same control for both Local and Cloud!
  const dropdownItems = useMemo<DropdownItem[]>(() => {
    if (isCloudHost) {
      const items: DropdownItem[] = [];
      const seen = new Set<string>();
      for (const r of cloudRepos) {
        const k = r.full_name.toLowerCase();
        if (seen.has(k)) continue;
        seen.add(k);
        items.push({
          key: `repo:${r.full_name}`,
          title: r.full_name,
          subtitle: r.cloned ? 'Cloned on cloud · ready' : 'Auto-clones on first message',
          cwd: r.cloned && r.path ? r.path : r.full_name,
          gitRemote: r.cloned ? undefined : r.git_remote,
          cloned: r.cloned,
          isPrivate: r.private,
        });
      }
      // Fallback to hostInfo.catalog_projects / projects while /v1/repos loads
      if (items.length === 0) {
        for (const c of hostInfo?.catalog_projects ?? []) {
          items.push({
            key: `cat:${c.name}`,
            title: c.name,
            subtitle: c.cloned ? 'Cloned on cloud · ready' : c.git_remote,
            cwd: c.cloned && c.path ? c.path : c.name,
            gitRemote: c.cloned ? undefined : c.git_remote,
            cloned: c.cloned,
          });
        }
        for (const p of hostInfo?.projects ?? []) {
          const short = p.split('/').filter(Boolean).pop() ?? p;
          if (short === 'workspaces' && items.length > 0) continue;
          if (!items.some((x) => x.title.toLowerCase() === short.toLowerCase())) {
            items.push({
              key: `dir:${p}`,
              title: short,
              subtitle: p,
              cwd: p,
              cloned: true,
            });
          }
        }
      }
      return items;
    }

    // Local host: existing local projects you already cloned & worked on
    return (hostInfo?.projects ?? []).map((p) => ({
      key: `local:${p}`,
      title: p.split('/').filter(Boolean).pop() ?? p,
      subtitle: p,
      cwd: p,
      cloned: true,
    }));
  }, [isCloudHost, cloudRepos, hostInfo?.projects, hostInfo?.catalog_projects]);

  useEffect(() => {
    if (!selectedItem && dropdownItems.length > 0) {
      setSelectedItem(dropdownItems[0]);
    }
  }, [dropdownItems, selectedItem]);

  const filteredItems = useMemo(() => {
    const q = pickerQuery.trim().toLowerCase();
    if (!q) return dropdownItems;
    const words = q.split(/\s+/).filter(Boolean);
    return dropdownItems.filter((item) => {
      const hay = `${item.title} ${item.subtitle}`.toLowerCase();
      return words.every((w) => hay.includes(w));
    });
  }, [dropdownItems, pickerQuery]);

  const clonedFiltered = useMemo(() => filteredItems.filter((i) => i.cloned), [filteredItems]);
  const remoteFiltered = useMemo(() => filteredItems.filter((i) => !i.cloned), [filteredItems]);

  // Load models/effort options for the active host.
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

  const effectiveCwd = selectedItem?.cwd ?? '';
  const effectiveGitRemote = selectedItem?.gitRemote;

  const currentModelLabel = useMemo(() => {
    if (!modelId) return 'Default model';
    const match = models.find((m) => m.id === modelId);
    return match?.name || modelId;
  }, [modelId, models]);

  const headerSubtitle = useMemo(() => {
    const hostLabel = activeHost?.name ?? 'Host';
    const projShort = selectedItem?.title ?? 'Select repository';
    return `${hostLabel} · ${projShort}`;
  }, [activeHost?.name, selectedItem?.title]);

  const handleSend = async (
    text: string,
    images: { data_base64: string; mime: string; name?: string }[],
  ) => {
    if (!transport || creating) return;
    if (!effectiveCwd) {
      setCreateError(
        isCloudHost ? 'Select a GitHub org/repo first.' : 'Select a project folder first.',
      );
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
        router.replace({
          pathname: '/chat/[id]',
          params: {
            id: encodeURIComponent(res.path),
            ...(activeHost?.id ? { hostId: activeHost.id } : {}),
          },
        });
        return;
      }

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
            router.replace({
              pathname: '/chat/[id]',
              params: {
                id: encodeURIComponent(newest.path),
                ...(activeHost?.id ? { hostId: activeHost.id } : {}),
              },
            });
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

  const renderDropdownRow = (item: DropdownItem) => {
    const active = selectedItem?.key === item.key;
    return (
      <Pressable
        key={item.key}
        onPress={() => {
          setSelectedItem(item);
          setPickerOpen(false);
          setPickerQuery('');
          setCreateError(null);
        }}
        style={[
          styles.pickerRow,
          {
            backgroundColor: active ? colors.accentBg : 'transparent',
            borderColor: colors.border,
          },
        ]}
      >
        <Ionicons
          name={item.cloned ? 'folder-outline' : 'git-branch-outline'}
          size={16}
          color={active ? colors.accent : colors.muted}
        />
        <View style={{ flex: 1, minWidth: 0 }}>
          <View style={{ flexDirection: 'row', alignItems: 'center', gap: 6 }}>
            <Text
              numberOfLines={1}
              style={{
                color: colors.text,
                fontSize: 13.5,
                fontWeight: active ? '700' : '500',
                flexShrink: 1,
              }}
            >
              {item.title}
            </Text>
            {item.isPrivate ? (
              <Ionicons name="lock-closed-outline" size={11} color={colors.dim} />
            ) : null}
          </View>
          <Text numberOfLines={1} style={{ color: colors.dim, fontSize: 11, marginTop: 1 }}>
            {item.subtitle}
          </Text>
        </View>
        {active ? <Ionicons name="checkmark" size={17} color={colors.accent} /> : null}
      </Pressable>
    );
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
              {isCloudHost
                ? 'Runs 24/7 in Auto mode inside an isolated pui_agent/… worktree and opens a PR when done.'
                : 'Select environment and project, then pick model, effort & permissions in the composer below.'}
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

          {/* Unified Searchable Dropdown for Project / Org/Repo */}
          <View style={styles.section}>
            <Text style={[styles.sectionLabel, { color: colors.muted }]}>
              {isCloudHost ? 'REPOSITORY (ORG/REPO)' : 'PROJECT'}
            </Text>
            <Pressable
              onPress={() => setPickerOpen(true)}
              style={[
                styles.dropdownTrigger,
                { backgroundColor: colors.surface, borderColor: colors.border },
              ]}
            >
              <Ionicons
                name={
                  selectedItem
                    ? selectedItem.cloned
                      ? 'folder-outline'
                      : 'git-branch-outline'
                    : isCloudHost
                      ? 'git-branch-outline'
                      : 'folder-outline'
                }
                size={16}
                color={colors.accent}
              />
              <View style={{ flex: 1, minWidth: 0 }}>
                <Text
                  numberOfLines={1}
                  style={{
                    color: selectedItem ? colors.text : colors.dim,
                    fontSize: 13.5,
                    fontWeight: '600',
                  }}
                >
                  {selectedItem
                    ? selectedItem.title
                    : reposLoading
                      ? 'Loading repositories…'
                      : isCloudHost
                        ? 'Search org/repo…'
                        : 'Select project…'}
                </Text>
                {selectedItem ? (
                  <Text numberOfLines={1} style={{ color: colors.dim, fontSize: 11, marginTop: 1 }}>
                    {selectedItem.subtitle}
                  </Text>
                ) : null}
              </View>
              {reposLoading ? (
                <ActivityIndicator size="small" color={colors.muted} />
              ) : (
                <Ionicons name="chevron-down" size={16} color={colors.muted} />
              )}
            </Pressable>
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
                  ? `Cloning ${selectedItem?.title} & creating worktree on ${activeHost?.name ?? 'cloud'}…`
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

      {/* Unified Searchable Project / Org/Repo Dropdown Modal */}
      <Modal
        visible={pickerOpen}
        transparent
        animationType="fade"
        onRequestClose={() => setPickerOpen(false)}
      >
        <View style={styles.pickerBackdrop}>
          <Pressable style={StyleSheet.absoluteFill} onPress={() => setPickerOpen(false)} />
          <View
            style={[
              styles.pickerCard,
              { backgroundColor: colors.surface, borderColor: colors.border },
            ]}
          >
            <View style={styles.pickerHeader}>
              <Text style={[styles.pickerTitle, { color: colors.text }]}>
                {isCloudHost ? 'Select repository' : 'Select project'}
              </Text>
              <View style={{ flexDirection: 'row', alignItems: 'center', gap: 10 }}>
                {isCloudHost ? (
                  <Pressable onPress={() => loadCloudRepos(true)} hitSlop={8}>
                    <Ionicons name="refresh-outline" size={18} color={colors.muted} />
                  </Pressable>
                ) : null}
                <Pressable onPress={() => setPickerOpen(false)} hitSlop={8}>
                  <Ionicons name="close" size={20} color={colors.muted} />
                </Pressable>
              </View>
            </View>

            <View
              style={[
                styles.pickerSearchBox,
                { backgroundColor: colors.codeBg, borderColor: colors.border },
              ]}
            >
              <Ionicons name="search-outline" size={16} color={colors.dim} />
              <TextInput
                autoFocus
                value={pickerQuery}
                onChangeText={setPickerQuery}
                placeholder={isCloudHost ? 'Search org/repo…' : 'Search existing projects…'}
                placeholderTextColor={colors.dim}
                autoCapitalize="none"
                autoCorrect={false}
                style={[styles.pickerSearchInput, { color: colors.text }]}
              />
              {pickerQuery ? (
                <Pressable onPress={() => setPickerQuery('')}>
                  <Ionicons name="close-circle" size={16} color={colors.dim} />
                </Pressable>
              ) : null}
            </View>

            <ScrollView style={{ maxHeight: 380 }} keyboardShouldPersistTaps="handled">
              {reposLoading && dropdownItems.length === 0 ? (
                <View style={styles.pickerEmpty}>
                  <ActivityIndicator size="small" color={colors.accent} />
                  <Text style={{ color: colors.muted, fontSize: 12.5 }}>
                    Loading GitHub repositories…
                  </Text>
                </View>
              ) : (
                <>
                  {clonedFiltered.length > 0 ? (
                    <>
                      <Text style={[styles.pickerGroupLabel, { color: colors.muted }]}>
                        {isCloudHost ? 'EXISTING ON CLOUD' : 'EXISTING PROJECTS'}
                      </Text>
                      {clonedFiltered.map(renderDropdownRow)}
                    </>
                  ) : null}

                  {remoteFiltered.length > 0 ? (
                    <>
                      <Text style={[styles.pickerGroupLabel, { color: colors.muted }]}>
                        GITHUB REPOSITORIES
                      </Text>
                      {remoteFiltered.map(renderDropdownRow)}
                    </>
                  ) : null}

                  {pickerQuery.trim() &&
                  ((isCloudHost && looksLikeGitRepo(pickerQuery.trim())) ||
                    (!isCloudHost && pickerQuery.trim().startsWith('/'))) ? (
                    <Pressable
                      onPress={() => {
                        const q = pickerQuery.trim();
                        setSelectedItem({
                          key: `custom:${q}`,
                          title: q,
                          subtitle: isCloudHost
                            ? 'Clone on demand into cloud workspaces'
                            : q,
                          cwd: q,
                          gitRemote: isCloudHost ? q : undefined,
                          cloned: !isCloudHost,
                        });
                        setPickerOpen(false);
                        setPickerQuery('');
                        setCreateError(null);
                      }}
                      style={[
                        styles.pickerRow,
                        { backgroundColor: colors.accentBg, borderColor: colors.border },
                      ]}
                    >
                      <Ionicons
                        name={isCloudHost ? 'cloud-download-outline' : 'folder-open-outline'}
                        size={16}
                        color={colors.accent}
                      />
                      <View style={{ flex: 1 }}>
                        <Text style={{ color: colors.text, fontSize: 13, fontWeight: '600' }}>
                          {isCloudHost ? `Clone & use ${pickerQuery.trim()}` : `Use ${pickerQuery.trim()}`}
                        </Text>
                      </View>
                    </Pressable>
                  ) : null}

                  {filteredItems.length === 0 && !reposLoading ? (
                    <View style={styles.pickerEmpty}>
                      <Text style={{ color: colors.muted, fontSize: 12.5, textAlign: 'center' }}>
                        {isCloudHost
                          ? 'No matching repositories. Type any org/repo (e.g. owner/repo) to clone.'
                          : 'No matching projects found.'}
                      </Text>
                    </View>
                  ) : null}
                </>
              )}
            </ScrollView>
          </View>
        </View>
      </Modal>

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
  dropdownTrigger: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    borderWidth: 1,
    borderRadius: 11,
    paddingHorizontal: 13,
    paddingVertical: 11,
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
  pickerBackdrop: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.55)',
    justifyContent: 'center',
    padding: 18,
  },
  pickerCard: {
    borderRadius: 14,
    borderWidth: 1,
    overflow: 'hidden',
    padding: 14,
    gap: 10,
  },
  pickerHeader: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
  },
  pickerTitle: {
    fontSize: 15,
    fontWeight: '700',
  },
  pickerSearchBox: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    borderWidth: 1,
    borderRadius: 10,
    paddingHorizontal: 10,
    height: 40,
  },
  pickerSearchInput: {
    flex: 1,
    fontSize: 13.5,
    paddingVertical: 0,
  },
  pickerGroupLabel: {
    fontSize: 10.5,
    fontWeight: '700',
    letterSpacing: 0.5,
    marginTop: 8,
    marginBottom: 4,
    paddingHorizontal: 4,
  },
  pickerRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    paddingHorizontal: 10,
    paddingVertical: 9,
    borderRadius: 9,
  },
  pickerEmpty: {
    paddingVertical: 24,
    alignItems: 'center',
    gap: 8,
  },
});
