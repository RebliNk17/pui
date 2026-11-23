import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ActivityIndicator,
  FlatList,
  Keyboard,
  KeyboardAvoidingView,
  Modal,
  Platform,
  Pressable,
  StyleSheet,
  Text,
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
import { groupTranscript } from '../../src/utils/transcript';

type Tab = 'chat' | 'artifacts' | 'files' | 'changes';

export default function ChatScreen() {
  const colors = usePalette();
  const { prefs, updatePrefs } = usePreferences();
  const params = useLocalSearchParams<{ id: string }>();
  const sessionPath = decodeURIComponent(params.id ?? '');

  const {
    detailsByPath,
    sessions,
    hostInfo,
    transport,
    clientOrigin,
    loadSessionDetail,
  } = usePui();

  const detail = detailsByPath[sessionPath];
  const summary = sessions.find((s) => s.path === sessionPath);
  const [tab, setTab] = useState<Tab>('chat');
  const [slashCommands, setSlashCommands] = useState<SlashCommandItem[]>([]);
  const [previewImage, setPreviewImage] = useState<{ uri: string; title?: string } | null>(null);
  const [fileToOpen, setFileToOpen] = useState<string | null>(null);
  const [usageOpen, setUsageOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [androidKeyboardHeight, setAndroidKeyboardHeight] = useState(0);
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

  const openFileInTree = (target: string) => {
    setFileToOpen(target);
    setTab('files');
  };

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
            if (tab !== 'chat') {
              setTab('chat');
            } else {
              router.back();
            }
          }}
          style={styles.iconBtn}
        >
          <Ionicons name="arrow-back" size={21} color={colors.text} />
        </Pressable>

        <View style={styles.titleCenter}>
          <Text numberOfLines={1} style={[styles.title, { color: colors.text }]}>
            {title}
          </Text>
          <Text numberOfLines={1} style={[styles.subText, { color: colors.muted }]}>
            {projectLabel}
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
                  contentContainerStyle={{ paddingVertical: 10 }}
                  renderItem={({ item }) => {
                    if (item.kind === 'activity') {
                      return (
                        <ActivityGroupRow
                          summary={item.summary}
                          tools={item.tools}
                          thinking={item.thinking}
                          hasRunning={item.hasRunning}
                          hasError={item.hasError}
                          verbose={verbose}
                          onOpenImage={(uri, imgTitle) => setPreviewImage({ uri, title: imgTitle })}
                        />
                      );
                    }
                    if (item.kind === 'user') {
                      return (
                        <MessageBubble
                          message={item.message}
                          onOpenImage={(uri, imgTitle) => setPreviewImage({ uri, title: imgTitle })}
                          onOpenFileTarget={openFileInTree}
                        />
                      );
                    }
                    return (
                      <MessageBubble
                        message={item.message}
                        inlineTools={item.inlineTools}
                        onOpenImage={(uri, imgTitle) => setPreviewImage({ uri, title: imgTitle })}
                        onOpenFileTarget={openFileInTree}
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
              cwd={cwd}
              busy={Boolean(detail?.busy)}
              permissionMode={detail?.permission_mode ?? 'auto'}
              permissionModes={permissionModes}
              currentModelId={detail?.model ?? ''}
              currentModelLabel={detail?.model_name || detail?.model || ''}
              currentThinking={detail?.thinking || prefs.defaultEffort}
              thinkingLevels={detail?.levels ?? []}
              models={detail?.models ?? []}
              slashCommands={slashCommands.length > 0 ? slashCommands : (detail?.commands ?? [])}
              waitingMessages={detail?.waiting_messages ?? []}
              onRemoveQueued={async (index) => {
                await transport?.removeQueuedMessage(sessionPath, index);
                setTimeout(() => void loadSessionDetail(sessionPath).catch(() => {}), 150);
              }}
              onSteerQueued={async (index) => {
                await transport?.steerQueuedMessage(sessionPath, index);
                setTimeout(() => void loadSessionDetail(sessionPath).catch(() => {}), 150);
              }}
              onFetchFileMentions={fetchMentions}
              onSend={async (text, images, mode) => {
                if (!transport) return;
                await transport.sendMessage({
                  path: sessionPath,
                  text,
                  images,
                  mode,
                  origin: clientOrigin(),
                });
                setTimeout(() => void loadSessionDetail(sessionPath).catch(() => {}), 180);
              }}
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
              onPreviewImage={(uri, imgTitle) => setPreviewImage({ uri, title: imgTitle })}
            />
          </>
        ) : tab === 'artifacts' ? (
          <ArtifactsView
            cwd={cwd}
            artifacts={artifacts}
            transport={transport}
            onOpenImage={(uri, imgTitle) => setPreviewImage({ uri, title: imgTitle })}
            onOpenFile={openFileInTree}
          />
        ) : tab === 'files' ? (
          <FileTreeView
            cwd={cwd}
            transport={transport}
            initialFilePath={fileToOpen}
            onOpenImage={(uri, imgTitle) => setPreviewImage({ uri, title: imgTitle })}
          />
        ) : (
          <ChangesView cwd={cwd} transport={transport} />
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
});
