import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
  ActivityIndicator,
  FlatList,
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
import { DialogCard } from '../../src/components/DialogCard';
import { FileTreeView } from '../../src/components/FileTreeView';
import { ImageModal } from '../../src/components/ImageModal';
import { MessageBubble } from '../../src/components/MessageBubble';
import { OriginBadge } from '../../src/components/OriginBadge';
import type { SlashCommandItem } from '../../src/protocol/types';
import { usePui } from '../../src/store/connectionStore';
import { usePalette } from '../../src/theme/colors';

type Tab = 'chat' | 'artifacts' | 'files' | 'changes';

export default function ChatScreen() {
  const colors = usePalette();
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
  const listRef = useRef<FlatList>(null);

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
  const origin = detail?.origin ?? summary?.origin;
  const artifacts = detail?.artifacts ?? [];
  const dialogs = detail?.dialogs ?? [];
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
      <View style={[styles.topBar, { backgroundColor: colors.surface, borderBottomColor: colors.border }]}>
        <Pressable onPress={() => router.back()} style={styles.backBtn}>
          <Ionicons name="chevron-back" size={22} color={colors.text} />
        </Pressable>
        <View style={{ flex: 1 }}>
          <Text numberOfLines={1} style={[styles.title, { color: colors.text }]}>
            {title}
          </Text>
          <View style={styles.subRow}>
            <OriginBadge origin={origin} compact />
            <Text numberOfLines={1} style={[styles.subText, { color: colors.muted }]}>
              {detail?.busy ? 'Working…' : detail?.status || summary?.status || 'Ready'}
            </Text>
          </View>
        </View>
        <Pressable
          onPress={() => void loadSessionDetail(sessionPath).catch(() => {})}
          style={styles.backBtn}
        >
          <Ionicons name="refresh" size={18} color={colors.muted} />
        </Pressable>
      </View>

      <View style={[styles.tabBar, { backgroundColor: colors.surface, borderBottomColor: colors.border }]}>
        {(
          [
            ['chat', 'Chat', 'chatbubble-outline'],
            ['artifacts', `Artifacts${artifacts.length ? ` (${artifacts.length})` : ''}`, 'layers-outline'],
            ['files', 'Files', 'folder-outline'],
            ['changes', 'Changes', 'git-compare-outline'],
          ] as [Tab, string, keyof typeof Ionicons.glyphMap][]
        ).map(([key, label, icon]) => {
          const active = tab === key;
          return (
            <Pressable
              key={key}
              onPress={() => setTab(key)}
              style={[
                styles.tabBtn,
                active && { borderBottomColor: colors.accent, borderBottomWidth: 2 },
              ]}
            >
              <Ionicons name={icon} size={14} color={active ? colors.accent : colors.muted} />
              <Text
                numberOfLines={1}
                style={[styles.tabLabel, { color: active ? colors.accent : colors.muted }]}
              >
                {label}
              </Text>
            </Pressable>
          );
        })}
      </View>

      <KeyboardAvoidingView
        style={{ flex: 1 }}
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
              <FlatList
                ref={listRef}
                data={detail.messages}
                keyExtractor={(item, idx) => `${item.role}-${item.timestamp}-${idx}`}
                contentContainerStyle={{ paddingVertical: 8 }}
                onContentSizeChange={() => listRef.current?.scrollToEnd({ animated: false })}
                renderItem={({ item }) => (
                  <MessageBubble
                    message={item}
                    onOpenImage={(uri, imgTitle) => setPreviewImage({ uri, title: imgTitle })}
                    onOpenFileTarget={openFileInTree}
                  />
                )}
                ListFooterComponent={
                  <>
                    {detail.pending_messages.map((pm) => (
                      <View
                        key={pm.id}
                        style={[
                          styles.pendingBox,
                          { backgroundColor: colors.surfaceElevated, borderColor: colors.border },
                        ]}
                      >
                        <Text style={{ color: colors.text, fontSize: 13 }}>{pm.text}</Text>
                        <Text style={{ color: colors.amber, fontSize: 11, marginTop: 3 }}>
                          {pm.status}
                        </Text>
                      </View>
                    ))}
                    {detail.busy ? (
                      <View style={styles.workingRow}>
                        <ActivityIndicator size="small" color={colors.accent} />
                        <Text style={{ color: colors.muted, fontSize: 12 }}>Pi is working…</Text>
                      </View>
                    ) : null}
                  </>
                }
              />
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
              currentModel={detail?.model_name || detail?.model || ''}
              models={detail?.models ?? []}
              slashCommands={slashCommands.length > 0 ? slashCommands : (detail?.commands ?? [])}
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
              onLocalCommand={(name) => {
                if (name === 'files') setTab('files');
              }}
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

      <ImageModal
        uri={previewImage?.uri ?? null}
        title={previewImage?.title}
        onClose={() => setPreviewImage(null)}
      />
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  safe: { flex: 1 },
  topBar: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingHorizontal: 8,
    paddingVertical: 8,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  backBtn: {
    padding: 6,
  },
  title: {
    fontSize: 15,
    fontWeight: '700',
  },
  subRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    marginTop: 2,
  },
  subText: {
    fontSize: 11.5,
    flex: 1,
  },
  tabBar: {
    flexDirection: 'row',
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  tabBtn: {
    flex: 1,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 5,
    paddingVertical: 10,
  },
  tabLabel: {
    fontSize: 12,
    fontWeight: '600',
  },
  loading: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
  },
  pendingBox: {
    marginHorizontal: 12,
    marginVertical: 4,
    padding: 10,
    borderRadius: 10,
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
