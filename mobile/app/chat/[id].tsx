import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ActivityIndicator,
  FlatList,
  Keyboard,
  KeyboardAvoidingView,
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
        <Pressable onPress={() => router.back()} style={styles.iconBtn}>
          <Ionicons name="arrow-back" size={21} color={colors.text} />
        </Pressable>

        <View style={styles.titleCenter}>
          <Text numberOfLines={1} style={[styles.title, { color: colors.text }]}>
            {title}
          </Text>
          <Text numberOfLines={1} style={[styles.subText, { color: colors.muted }]}>
            {projectLabel}
            {detail?.busy ? ' · Working…' : ''}
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
            onPress={() =>
              updatePrefs({
                transcriptView: verbose ? 'normal' : 'verbose',
              })
            }
            style={styles.iconBtn}
            accessibilityLabel="Toggle verbose transcript"
          >
            <Ionicons
              name={verbose ? 'eye' : 'eye-outline'}
              size={19}
              color={verbose ? colors.accent : colors.muted}
            />
          </Pressable>
        </View>
      </View>

      {/* Compact pill switcher */}
      <View style={[styles.tabBar, { borderBottomColor: colors.hairline }]}>
        {(
          [
            ['chat', 'Chat'],
            ['artifacts', artifacts.length ? `Artifacts (${artifacts.length})` : 'Artifacts'],
            ['files', 'Files'],
            ['changes', 'Changes'],
          ] as [Tab, string][]
        ).map(([key, label]) => {
          const active = tab === key;
          return (
            <Pressable
              key={key}
              onPress={() => setTab(key)}
              style={[
                styles.tabPill,
                {
                  backgroundColor: active ? colors.surfaceElevated : 'transparent',
                  borderColor: active ? colors.border : 'transparent',
                },
              ]}
            >
              <Text
                numberOfLines={1}
                style={[
                  styles.tabLabel,
                  { color: active ? colors.text : colors.muted },
                ]}
              >
                {label}
              </Text>
            </Pressable>
          );
        })}
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
              }}
              onAbort={async () => {
                await transport?.abortSession(sessionPath);
              }}
              onChangePermission={async (key) => {
                await transport?.updateSessionSettings({
                  path: sessionPath,
                  permission_mode: key,
                });
              }}
              onChangeModel={async (provider, model) => {
                await transport?.updateSessionSettings({
                  path: sessionPath,
                  provider,
                  model,
                });
              }}
              onChangeThinking={async (thinking) => {
                await transport?.updateSessionSettings({
                  path: sessionPath,
                  thinking,
                });
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

      <UsageSheet
        visible={usageOpen}
        detail={detail}
        onClose={() => setUsageOpen(false)}
        onRefresh={() => void loadSessionDetail(sessionPath).catch(() => {})}
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
  tabBar: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    paddingHorizontal: 12,
    paddingVertical: 7,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  tabPill: {
    paddingHorizontal: 11,
    paddingVertical: 5,
    borderRadius: 999,
    borderWidth: StyleSheet.hairlineWidth,
  },
  tabLabel: {
    fontSize: 12,
    fontWeight: '600',
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
