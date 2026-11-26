import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ActivityIndicator,
  BackHandler,
  FlatList,
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
import { router, useLocalSearchParams } from 'expo-router';
import { Ionicons } from '@expo/vector-icons';
import { ArtifactsView } from '../../src/components/ArtifactsView';
import { ChangesView } from '../../src/components/ChangesView';
import { Composer } from '../../src/components/Composer';
import { ContextRing } from '../../src/components/ContextRing';
import { CustomScrollbarOverlay, useCustomScrollbar } from '../../src/components/CustomScrollbar';
import { DialogCard } from '../../src/components/DialogCard';
import { FileTreeView } from '../../src/components/FileTreeView';
import { ImageModal } from '../../src/components/ImageModal';
import { ActivityGroupRow, MessageBubble } from '../../src/components/MessageBubble';
import { UsageSheet } from '../../src/components/UsageSheet';
import type { SlashCommandItem } from '../../src/protocol/types';
import { usePui } from '../../src/store/connectionStore';
import { usePreferences } from '../../src/store/preferencesStore';
import { usePalette } from '../../src/theme/colors';
import { findMatchingEntryIndices } from '../../src/utils/search';
import {
  collectChatGalleryImages,
  type GalleryImage,
  isImageFilePath,
} from '../../src/utils/imageZoom';
import { descendantsOfSession } from '../../src/utils/sessions';
import { groupTranscript } from '../../src/utils/transcript';

type Tab = 'chat' | 'artifacts' | 'files' | 'changes';

export default function ChatScreen() {
  const colors = usePalette();
  const { prefs, updatePrefs } = usePreferences();
  const params = useLocalSearchParams<{ id: string; hostId?: string }>();
  const sessionPath = decodeURIComponent(params.id ?? '');

  const {
    detailsByPath,
    sessions,
    getTransportForHost,
    getHostInfoForHost,
    clientOrigin,
    loadSessionDetail: loadSessionDetailFromStore,
    sendMessage: sendMessageFromStore,
    removeQueuedMessage: removeQueuedMessageFromStore,
    setActiveChatPath,
    pendingShare,
    setPendingShare,
  } = usePui();

  const summary = sessions.find((s) => s.path === sessionPath);
  const resolvedHostId = params.hostId || summary?.host_id || null;
  const transport = getTransportForHost(resolvedHostId);
  const hostInfo = getHostInfoForHost(resolvedHostId);
  const loadSessionDetail = useCallback(
    (path: string) => loadSessionDetailFromStore(path, resolvedHostId),
    [loadSessionDetailFromStore, resolvedHostId],
  );

  useEffect(() => {
    setActiveChatPath(sessionPath || null);
    return () => setActiveChatPath(null);
  }, [sessionPath, setActiveChatPath]);

  const detail = detailsByPath[sessionPath];
  const [tab, setTab] = useState<Tab>('chat');
  const [slashCommands, setSlashCommands] = useState<SlashCommandItem[]>([]);
  const [previewGallery, setPreviewGallery] = useState<{
    images: GalleryImage[];
    initialIndex: number;
  } | null>(null);
  const [fileToOpen, setFileToOpen] = useState<string | null>(null);
  const [usageOpen, setUsageOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState('');
  const [findCursor, setFindCursor] = useState(0);
  const [composerPrefill, setComposerPrefill] = useState<{
    id: number;
    text?: string;
    imageBase64?: string;
    imageMime?: string;
  } | null>(null);
  const [androidKeyboardHeight, setAndroidKeyboardHeight] = useState(0);

  // Consume any incoming Android Share Sheet payload when entering this chat.
  useEffect(() => {
    if (!pendingShare) return;
    setComposerPrefill({
      id: Date.now(),
      text: pendingShare.text,
      imageBase64: pendingShare.imageBase64,
      imageMime: pendingShare.imageMime,
    });
    setPendingShare(null);
    setTab('chat');
  }, [pendingShare, setPendingShare]);
  const listRef = useRef<FlatList>(null);
  const { bindings, metrics } = useCustomScrollbar();

  // Explicit Android keyboard height listener so the floating composer ALWAYS rises cleanly
  // above the software keyboard even under Android 15+/Expo edge-to-edge mode.
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

  useEffect(() => {
    if (!sessionPath || !transport) return;
    transport.subscribeSession(sessionPath);
    void loadSessionDetail(sessionPath).catch(() => {});
    transport
      .getSlashCommands(sessionPath)
      .then(setSlashCommands)
      .catch(() => {});
    return () => {
      transport.subscribeSession(null);
    };
  }, [sessionPath, transport, loadSessionDetail]);

  const fetchMentions = useCallback(
    async (cwd: string, q: string) => {
      if (!transport) return [];
      return transport.getFileMentions(cwd, q);
    },
    [transport],
  );

  const cwd = detail?.cwd || summary?.cwd || '';
  const title = detail?.title || summary?.title || 'Conversation';
  const projectLabel = summary?.project_name || cwd.split('/').filter(Boolean).pop() || '';
  const artifacts = detail?.artifacts ?? [];
  const dialogs = detail?.dialogs ?? [];
  // Inverted FlatList displays index 0 at the bottom so chats open immediately at the latest
  // message without scrolling down through history from the top.
  const reversedEntries = useMemo(
    () => groupTranscript(detail?.messages ?? []).reverse(),
    [detail?.messages],
  );
  const childAgents = useMemo(
    () => descendantsOfSession(sessions, sessionPath),
    [sessions, sessionPath],
  );
  const findMatches = useMemo(
    () => (findOpen ? findMatchingEntryIndices(reversedEntries, findQuery) : []),
    [findOpen, reversedEntries, findQuery],
  );
  const activeMatchIndex =
    findMatches.length > 0 ? findMatches[findCursor % findMatches.length] : -1;

  useEffect(() => {
    setFindCursor(0);
  }, [findQuery]);

  const jumpToMatch = (nextCursor: number) => {
    if (findMatches.length === 0) return;
    const normalized = ((nextCursor % findMatches.length) + findMatches.length) % findMatches.length;
    setFindCursor(normalized);
    const targetIdx = findMatches[normalized];
    try {
      listRef.current?.scrollToIndex({ index: targetIdx, animated: true, viewPosition: 0.5 });
    } catch {
      // ignore if item hasn't measured yet
    }
  };

  const prefillInChat = (promptText: string) => {
    setTab('chat');
    setComposerPrefill({ id: Date.now(), text: promptText });
  };
  const contextProgress =
    detail && detail.context_window > 0 && detail.context_tokens != null
      ? detail.context_tokens / detail.context_window
      : 0;
  const verbose = prefs.transcriptView === 'verbose';
  const permissionModes = hostInfo?.permission_modes ?? [
    { key: 'auto', label: 'Auto' },
    { key: 'manual', label: 'Manual' },
    { key: 'acceptEdits', label: 'Accept edits' },
    { key: 'plan', label: 'Plan' },
    { key: 'dontAsk', label: "Don't ask" },
    { key: 'bypassPermissions', label: 'Bypass' },
  ];

  const resolveChatImageUri = useCallback(
    (src: string): string | null => {
      const trimmed = src.trim();
      if (!trimmed) return null;
      if (/^(https?:\/\/|data:image\/)/i.test(trimmed)) return trimmed;
      if (!transport) return null;
      return transport.getRawFileUrl(cwd, trimmed);
    },
    [transport, cwd],
  );

  const chatGallery = useMemo(
    () => collectChatGalleryImages(detail?.messages ?? [], resolveChatImageUri),
    [detail?.messages, resolveChatImageUri],
  );

  const openGalleryImage = useCallback(
    (uri: string, imgTitle?: string) => {
      const idx = chatGallery.findIndex((item) => item.uri === uri);
      if (idx >= 0) {
        setPreviewGallery({ images: chatGallery, initialIndex: idx });
      } else {
        setPreviewGallery({ images: [{ uri, title: imgTitle }], initialIndex: 0 });
      }
    },
    [chatGallery],
  );

  const openFileInTree = (target: string) => {
    if (transport && isImageFilePath(target)) {
      const label = target.split('/').pop() || target;
      openGalleryImage(transport.getRawFileUrl(cwd, target), label);
      return;
    }
    setFileToOpen(target);
    setTab('files');
  };

  const handleNavigateBack = useCallback((): boolean => {
    if (previewGallery) {
      setPreviewGallery(null);
      return true;
    }
    if (menuOpen) {
      setMenuOpen(false);
      return true;
    }
    if (usageOpen) {
      setUsageOpen(false);
      return true;
    }
    if (tab !== 'chat') {
      setFileToOpen(null);
      setTab('chat');
      return true;
    }
    if (findOpen) {
      setFindOpen(false);
      setFindQuery('');
      return true;
    }
    return false;
  }, [previewGallery, menuOpen, usageOpen, tab, findOpen]);

  // Intercept Android swipe-back / hardware back so leaving Files/Artifacts/Changes
  // returns to the chat that opened them instead of popping all the way to the chat list.
  useEffect(() => {
    const sub = BackHandler.addEventListener('hardwareBackPress', handleNavigateBack);
    return () => sub.remove();
  }, [handleNavigateBack]);

  return (
    <SafeAreaView style={[styles.safe, { backgroundColor: colors.bg }]}>
      {/* Sleek LTR top bar */}
      <View
        style={[
          styles.topBar,
          { backgroundColor: colors.bg, borderBottomColor: colors.hairline },
        ]}
      >
        <Pressable
          onPress={() => {
            if (!handleNavigateBack()) {
              router.back();
            }
          }}
          style={styles.iconBtn}
        >
          <Ionicons name="arrow-back" size={21} color={colors.text} />
        </Pressable>

        <View style={styles.titleCenter}>
          <View style={{ flexDirection: 'row', alignItems: 'center', gap: 5, maxWidth: '100%' }}>
            {(summary?.host_kind === 'cloud' || hostInfo?.pui_version?.toLowerCase().includes('cloud')) ? (
              <Ionicons name="cloud-outline" size={14} color={colors.accent} />
            ) : null}
            <Text numberOfLines={1} style={[styles.title, { color: colors.text, flexShrink: 1 }]}>
              {title}
            </Text>
          </View>
          <Text numberOfLines={1} style={[styles.subText, { color: colors.muted }]}>
            {summary?.parent_session ? `Subagent · ${projectLabel}` : projectLabel}
            {tab !== 'chat'
              ? ` · ${tab === 'artifacts' ? 'Artifacts' : tab === 'files' ? 'Files' : 'Changes'}`
              : detail?.busy
                ? ' · Working…'
                : ''}
          </Text>
        </View>

        <View style={styles.topRight}>
          <Pressable
            onPress={() => setUsageOpen(true)}
            style={styles.iconBtn}
            accessibilityLabel="Context and usage"
          >
            <ContextRing progress={contextProgress} size={22} />
          </Pressable>
          <Pressable
            onPress={() => setMenuOpen(true)}
            style={styles.iconBtn}
            accessibilityLabel="Session menu"
          >
            <Ionicons name="ellipsis-vertical" size={19} color={colors.muted} />
          </Pressable>
        </View>
      </View>

      {/* Subagents bar (when this chat has spawned child workers) */}
      {tab === 'chat' && childAgents.length > 0 ? (
        <ScrollView
          horizontal
          showsHorizontalScrollIndicator={false}
          style={[styles.subagentBar, { borderBottomColor: colors.hairline, backgroundColor: colors.surface }]}
          contentContainerStyle={styles.subagentBarContent}
        >
          <Ionicons name="git-branch-outline" size={13} color={colors.muted} />
          <Text style={{ color: colors.dim, fontSize: 11, fontWeight: '600' }}>
            Subagents ({childAgents.length})
          </Text>
          {childAgents.map((child) => (
            <Pressable
              key={child.path}
              onPress={() => {
                void transport?.openSession(child.path).catch(() => {});
                router.push({
                  pathname: '/chat/[id]',
                  params: {
                    id: encodeURIComponent(child.path),
                    ...(resolvedHostId ? { hostId: resolvedHostId } : {}),
                  },
                });
              }}
              style={[
                styles.subagentChip,
                { backgroundColor: colors.bg, borderColor: colors.border },
              ]}
            >
              <View
                style={[
                  styles.subagentDot,
                  {
                    backgroundColor: child.has_pending_dialog
                      ? colors.amber
                      : child.busy
                        ? colors.accent
                        : colors.green,
                  },
                ]}
              />
              <Text numberOfLines={1} style={{ color: colors.text, fontSize: 11.5, maxWidth: 140 }}>
                {child.title}
              </Text>
            </Pressable>
          ))}
        </ScrollView>
      ) : null}

      {/* In-chat Find bar */}
      {tab === 'chat' && findOpen ? (
        <View
          style={[
            styles.findBar,
            { backgroundColor: colors.surface, borderBottomColor: colors.border },
          ]}
        >
          <Ionicons name="search-outline" size={15} color={colors.dim} />
          <TextInput
            autoFocus
            value={findQuery}
            onChangeText={setFindQuery}
            placeholder="Find in chat…"
            placeholderTextColor={colors.dim}
            style={[styles.findInput, { color: colors.text }]}
          />
          {findQuery.trim() ? (
            <Text style={{ color: colors.muted, fontSize: 11.5 }}>
              {findMatches.length > 0 ? `${(findCursor % findMatches.length) + 1}/${findMatches.length}` : '0'}
            </Text>
          ) : null}
          <Pressable
            onPress={() => jumpToMatch(findCursor + 1)}
            disabled={findMatches.length === 0}
            hitSlop={6}
            style={{ opacity: findMatches.length > 0 ? 1 : 0.35 }}
          >
            <Ionicons name="chevron-up" size={18} color={colors.text} />
          </Pressable>
          <Pressable
            onPress={() => jumpToMatch(findCursor - 1)}
            disabled={findMatches.length === 0}
            hitSlop={6}
            style={{ opacity: findMatches.length > 0 ? 1 : 0.35 }}
          >
            <Ionicons name="chevron-down" size={18} color={colors.text} />
          </Pressable>
          <Pressable
            onPress={() => {
              setFindOpen(false);
              setFindQuery('');
            }}
            hitSlop={6}
          >
            <Ionicons name="close" size={18} color={colors.muted} />
          </Pressable>
        </View>
      ) : null}

      <KeyboardAvoidingView
        style={{ flex: 1, paddingBottom: Platform.OS === 'android' ? androidKeyboardHeight : 0 }}
        behavior={Platform.OS === 'ios' ? 'padding' : undefined}
      >
        {tab === 'chat' ? (
          <>
            {!detail ? (
              <View style={styles.loading}>
                <ActivityIndicator color={colors.accent} />
                <Text style={{ color: colors.muted, marginTop: 8, fontSize: 12.5 }}>
                  Loading conversation from Pui host…
                </Text>
              </View>
            ) : (
              <View style={styles.listWrap}>
                <FlatList
                  ref={listRef}
                  {...bindings}
                  inverted
                  data={reversedEntries}
                  keyExtractor={(item) => item.key}
                  initialNumToRender={12}
                  maxToRenderPerBatch={10}
                  windowSize={7}
                  onScrollToIndexFailed={({ index }) => {
                    listRef.current?.scrollToOffset({ offset: index * 120, animated: true });
                  }}
                  contentContainerStyle={{ paddingVertical: 10 }}
                  renderItem={({ item, index }) => {
                    const isHit = index === activeMatchIndex;
                    if (item.kind === 'activity') {
                      return (
                        <ActivityGroupRow
                          summary={item.summary}
                          tools={item.tools}
                          thinking={item.thinking}
                          hasRunning={item.hasRunning}
                          hasError={item.hasError}
                          verbose={verbose}
                          onOpenImage={openGalleryImage}
                        />
                      );
                    }
                    if (item.kind === 'user') {
                      return (
                        <MessageBubble
                          message={item.message}
                          highlighted={isHit}
                          onOpenImage={openGalleryImage}
                          onOpenFileTarget={openFileInTree}
                          resolveImageUri={resolveChatImageUri}
                        />
                      );
                    }
                    return (
                      <MessageBubble
                        message={item.message}
                        inlineTools={item.inlineTools}
                        highlighted={isHit}
                        onOpenImage={openGalleryImage}
                        onOpenFileTarget={openFileInTree}
                        resolveImageUri={resolveChatImageUri}
                      />
                    );
                  }}
                  ListHeaderComponent={
                    <>
                      {detail.busy ? (
                        <View style={styles.workingRow}>
                          <ActivityIndicator size="small" color={colors.accent} />
                          <Text style={{ color: colors.muted, fontSize: 12 }}>Pi is working…</Text>
                        </View>
                      ) : null}
                      {detail.pending_messages.map((pm) => (
                        <View
                          key={pm.id}
                          style={[
                            styles.pendingBox,
                            { backgroundColor: colors.surface, borderColor: colors.border },
                          ]}
                        >
                          <Text style={{ color: colors.text, fontSize: 13.5, textAlign: 'left' }}>
                            {pm.text}
                          </Text>
                          <Text
                            style={{
                              color: colors.amber,
                              fontSize: 11,
                              marginTop: 4,
                              textAlign: 'left',
                            }}
                          >
                            {pm.status}
                          </Text>
                        </View>
                      ))}
                    </>
                  }
                />
                <CustomScrollbarOverlay {...metrics} inverted />
                {metrics.offsetY > 140 ? (
                  <Pressable
                    onPress={() => listRef.current?.scrollToOffset({ offset: 0, animated: true })}
                    accessibilityLabel="Scroll to bottom"
                    style={({ pressed }) => [
                      styles.scrollBottomFab,
                      {
                        backgroundColor: colors.surfaceElevated,
                        borderColor: colors.border,
                        opacity: pressed ? 0.8 : 1,
                      },
                    ]}
                  >
                    <Ionicons name="arrow-down" size={18} color={colors.text} />
                  </Pressable>
                ) : null}
              </View>
            )}

            {dialogs.length > 0 && transport ? (
              <DialogCard
                dialog={dialogs[0]}
                onRespond={async (dialogId, response) => {
                  await transport.answerDialog({
                    path: sessionPath,
                    dialog_id: dialogId,
                    response,
                  });
                }}
              />
            ) : null}

            <Composer
              sessionPath={sessionPath}
              cwd={cwd}
              busy={Boolean(detail?.busy)}
              prefill={composerPrefill}
              permissionMode={detail?.permission_mode ?? 'auto'}
              permissionModes={permissionModes}
              currentModelId={detail?.model ?? ''}
              currentModelLabel={detail?.model_name || detail?.model || ''}
              currentThinking={detail?.thinking || prefs.defaultEffort}
              thinkingLevels={detail?.levels ?? []}
              models={detail?.models ?? []}
              slashCommands={slashCommands.length > 0 ? slashCommands : (detail?.commands ?? [])}
              waitingMessages={detail?.waiting_messages ?? []}
              onRemoveQueued={(index) =>
                removeQueuedMessageFromStore(sessionPath, resolvedHostId, index)
              }
              onSteerQueued={async (index) => {
                await transport?.steerQueuedMessage(sessionPath, index);
                setTimeout(() => void loadSessionDetail(sessionPath).catch(() => {}), 150);
              }}
              onFetchFileMentions={fetchMentions}
              onSend={(text, images, mode) =>
                sendMessageFromStore(sessionPath, resolvedHostId, {
                  text,
                  images,
                  mode,
                  origin: clientOrigin(),
                })
              }
              onAbort={async () => {
                await transport?.abortSession(sessionPath);
              }}
              onChangePermission={async (key) => {
                await transport?.updateSessionSettings({
                  path: sessionPath,
                  permission_mode: key,
                });
                setTimeout(() => void loadSessionDetail(sessionPath).catch(() => {}), 180);
              }}
              onChangeModel={async (provider, model) => {
                await transport?.updateSessionSettings({
                  path: sessionPath,
                  provider,
                  model,
                });
                setTimeout(() => void loadSessionDetail(sessionPath).catch(() => {}), 180);
              }}
              onChangeThinking={async (thinking) => {
                updatePrefs({ defaultEffort: thinking });
                await transport?.updateSessionSettings({
                  path: sessionPath,
                  thinking,
                });
                setTimeout(() => void loadSessionDetail(sessionPath).catch(() => {}), 180);
              }}
              onLocalCommand={(name) => {
                if (name === 'files') setTab('files');
              }}
              onPreviewImage={openGalleryImage}
            />
          </>
        ) : tab === 'artifacts' ? (
          <ArtifactsView
            cwd={cwd}
            artifacts={artifacts}
            transport={transport}
            onOpenImage={openGalleryImage}
            onOpenFile={openFileInTree}
          />
        ) : tab === 'files' ? (
          <FileTreeView
            cwd={cwd}
            transport={transport}
            initialFilePath={fileToOpen}
            onOpenImage={openGalleryImage}
            onCloseDirectFile={
              fileToOpen
                ? () => {
                    setFileToOpen(null);
                    setTab('chat');
                  }
                : undefined
            }
          />
        ) : (
          <ChangesView cwd={cwd} transport={transport} onAskPi={prefillInChat} />
        )}
      </KeyboardAvoidingView>

      <Modal
        visible={menuOpen}
        transparent
        animationType="fade"
        onRequestClose={() => setMenuOpen(false)}
      >
        <Pressable style={styles.menuBackdrop} onPress={() => setMenuOpen(false)}>
          <Pressable
            onPress={(e) => e.stopPropagation()}
            style={[
              styles.menuCard,
              { backgroundColor: colors.surface, borderColor: colors.border },
            ]}
          >
            {(
              [
                ['chat', 'Chat', 'chatbubble-outline', null],
                [
                  'artifacts',
                  'Artifacts',
                  'layers-outline',
                  artifacts.length > 0 ? String(artifacts.length) : null,
                ],
                ['files', 'Files', 'folder-outline', null],
                ['changes', 'Changes', 'git-compare-outline', null],
              ] as [Tab, string, keyof typeof Ionicons.glyphMap, string | null][]
            ).map(([key, label, icon, badge]) => {
              const active = tab === key;
              return (
                <Pressable
                  key={key}
                  onPress={() => {
                    setTab(key);
                    setMenuOpen(false);
                  }}
                  style={[
                    styles.menuRow,
                    active && { backgroundColor: colors.accentBg },
                  ]}
                >
                  <Ionicons
                    name={icon}
                    size={17}
                    color={active ? colors.accent : colors.muted}
                  />
                  <Text
                    style={[
                      styles.menuLabel,
                      { color: active ? colors.accent : colors.text },
                    ]}
                  >
                    {label}
                  </Text>
                  {badge ? (
                    <View
                      style={[
                        styles.menuBadge,
                        { backgroundColor: colors.surfaceElevated },
                      ]}
                    >
                      <Text style={{ color: colors.muted, fontSize: 11, fontWeight: '600' }}>
                        {badge}
                      </Text>
                    </View>
                  ) : null}
                  {active ? (
                    <Ionicons name="checkmark" size={16} color={colors.accent} />
                  ) : null}
                </Pressable>
              );
            })}

            <View style={[styles.menuDivider, { backgroundColor: colors.hairline }]} />

            <Pressable
              onPress={() => {
                setTab('chat');
                setFindOpen(true);
                setMenuOpen(false);
              }}
              style={styles.menuRow}
            >
              <Ionicons name="search-outline" size={17} color={colors.muted} />
              <Text style={[styles.menuLabel, { color: colors.text }]}>Find in chat</Text>
            </Pressable>

            <Pressable
              onPress={() => {
                updatePrefs({
                  transcriptView: verbose ? 'normal' : 'verbose',
                });
                setMenuOpen(false);
              }}
              style={styles.menuRow}
            >
              <Ionicons
                name={verbose ? 'eye' : 'eye-outline'}
                size={17}
                color={verbose ? colors.accent : colors.muted}
              />
              <Text style={[styles.menuLabel, { color: colors.text }]}>
                Verbose transcript
              </Text>
              <View
                style={[
                  styles.menuBadge,
                  {
                    backgroundColor: verbose ? colors.accentBg : colors.surfaceElevated,
                  },
                ]}
              >
                <Text
                  style={{
                    color: verbose ? colors.accent : colors.muted,
                    fontSize: 11,
                    fontWeight: '600',
                  }}
                >
                  {verbose ? 'On' : 'Off'}
                </Text>
              </View>
            </Pressable>
          </Pressable>
        </Pressable>
      </Modal>

      <UsageSheet
        visible={usageOpen}
        detail={detail}
        transport={transport}
        onClose={() => setUsageOpen(false)}
        onRefreshSession={() => void loadSessionDetail(sessionPath).catch(() => {})}
      />

      <ImageModal
        images={previewGallery?.images}
        initialIndex={previewGallery?.initialIndex}
        onClose={() => setPreviewGallery(null)}
      />
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  safe: {
    flex: 1,
    direction: 'ltr',
  },
  topBar: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    paddingHorizontal: 8,
    paddingVertical: 8,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  iconBtn: {
    width: 36,
    height: 36,
    alignItems: 'center',
    justifyContent: 'center',
  },
  titleCenter: {
    flex: 1,
    alignItems: 'center',
    minWidth: 0,
  },
  title: {
    fontSize: 15,
    fontWeight: '700',
  },
  subText: {
    fontSize: 11.5,
    marginTop: 1,
  },
  topRight: {
    flexDirection: 'row',
    alignItems: 'center',
  },
  menuBackdrop: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.35)',
    alignItems: 'flex-end',
    paddingTop: 58,
    paddingRight: 12,
  },
  menuCard: {
    width: 224,
    borderRadius: 14,
    borderWidth: 1,
    paddingVertical: 6,
    shadowColor: '#000',
    shadowOpacity: 0.25,
    shadowRadius: 12,
    shadowOffset: { width: 0, height: 6 },
    elevation: 10,
    direction: 'ltr',
  },
  menuRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    paddingHorizontal: 14,
    paddingVertical: 11,
    marginHorizontal: 4,
    borderRadius: 9,
  },
  menuLabel: {
    flex: 1,
    fontSize: 14,
    fontWeight: '500',
    textAlign: 'left',
  },
  menuBadge: {
    paddingHorizontal: 7,
    paddingVertical: 2,
    borderRadius: 999,
  },
  menuDivider: {
    height: StyleSheet.hairlineWidth,
    marginVertical: 5,
    marginHorizontal: 12,
  },
  listWrap: {
    flex: 1,
    position: 'relative',
  },
  scrollBottomFab: {
    position: 'absolute',
    right: 16,
    bottom: 12,
    width: 38,
    height: 38,
    borderRadius: 19,
    borderWidth: 1,
    alignItems: 'center',
    justifyContent: 'center',
    shadowColor: '#000',
    shadowOpacity: 0.3,
    shadowRadius: 8,
    shadowOffset: { width: 0, height: 3 },
    elevation: 6,
  },
  loading: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
  },
  pendingBox: {
    marginHorizontal: 14,
    marginVertical: 4,
    padding: 11,
    borderRadius: 12,
    borderWidth: 1,
  },
  workingRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingHorizontal: 16,
    paddingVertical: 10,
  },
  subagentBar: {
    flexGrow: 0,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  subagentBarContent: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 7,
    paddingHorizontal: 12,
    paddingVertical: 7,
  },
  subagentChip: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    paddingHorizontal: 9,
    paddingVertical: 4,
    borderRadius: 999,
    borderWidth: StyleSheet.hairlineWidth,
  },
  subagentDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
  },
  findBar: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingHorizontal: 12,
    paddingVertical: 6,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  findInput: {
    flex: 1,
    fontSize: 13.5,
    paddingVertical: 4,
    textAlign: 'left',
  },
});
