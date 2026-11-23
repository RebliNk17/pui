import React, { useEffect, useMemo, useState } from 'react';
import {
  FlatList,
  Image,
  Modal,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import * as ImagePicker from 'expo-image-picker';
import type {
  FileMentionItem,
  ModelItem,
  PermissionModeOption,
  SlashCommandItem,
} from '../protocol/types';
import { usePalette } from '../theme/colors';
import {
  applyCompletion,
  detectCompletionToken,
  filterFileMentions,
  filterSlashCommands,
} from '../utils/completions';

interface DraftImage {
  uri: string;
  data_base64: string;
  mime: string;
  name?: string;
}

interface Props {
  cwd: string;
  busy: boolean;
  permissionMode: string;
  permissionModes: PermissionModeOption[];
  currentModel: string;
  models: ModelItem[];
  slashCommands: SlashCommandItem[];
  onFetchFileMentions: (cwd: string, query: string) => Promise<FileMentionItem[]>;
  onSend: (
    text: string,
    images: { data_base64: string; mime: string; name?: string }[],
    mode: 'steer' | 'queue',
  ) => Promise<void>;
  onAbort: () => Promise<void>;
  onChangePermission: (key: string) => Promise<void>;
  onChangeModel: (provider: string, modelId: string) => Promise<void>;
  onLocalCommand?: (name: string) => void;
}

export function Composer({
  cwd,
  busy,
  permissionMode,
  permissionModes,
  currentModel,
  models,
  slashCommands,
  onFetchFileMentions,
  onSend,
  onAbort,
  onChangePermission,
  onChangeModel,
  onLocalCommand,
}: Props) {
  const colors = usePalette();
  const [text, setText] = useState('');
  const [selection, setSelection] = useState({ start: 0, end: 0 });
  const [images, setImages] = useState<DraftImage[]>([]);
  const [fileResults, setFileResults] = useState<FileMentionItem[]>([]);
  const [dismissedAt, setDismissedAt] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [showPermPicker, setShowPermPicker] = useState(false);
  const [showModelPicker, setShowModelPicker] = useState(false);

  const token = useMemo(
    () => detectCompletionToken(text, selection.start),
    [text, selection.start],
  );
  const tokenKey = token ? `${token.kind}:${token.start}:${token.query}` : null;
  const completionVisible = Boolean(token && tokenKey !== dismissedAt);

  useEffect(() => {
    if (!token || token.kind !== 'file' || !cwd) {
      setFileResults([]);
      return;
    }
    let cancelled = false;
    const handle = setTimeout(() => {
      onFetchFileMentions(cwd, token.query)
        .then((items) => {
          if (!cancelled) setFileResults(items);
        })
        .catch(() => {
          if (!cancelled) setFileResults([]);
        });
    }, 120);
    return () => {
      cancelled = true;
      clearTimeout(handle);
    };
  }, [token?.kind, token?.query, cwd, onFetchFileMentions]);

  const commandChoices = useMemo(
    () => (token?.kind === 'command' ? filterSlashCommands(slashCommands, token.query) : []),
    [token?.kind, token?.query, slashCommands],
  );

  const fileChoices = useMemo(
    () => (token?.kind === 'file' ? filterFileMentions(fileResults, token.query) : []),
    [token?.kind, token?.query, fileResults],
  );

  const pickCompletion = (value: string) => {
    if (!token) return;
    const applied = applyCompletion(text, token, value);
    setText(applied.text);
    setSelection({ start: applied.cursor, end: applied.cursor });
    setDismissedAt(null);
  };

  const attachImage = async () => {
    const res = await ImagePicker.launchImageLibraryAsync({
      mediaTypes: ['images'],
      base64: true,
      quality: 0.85,
    });
    if (res.canceled || !res.assets?.[0]?.base64) return;
    const asset = res.assets[0];
    setImages((prev) => [
      ...prev,
      {
        uri: asset.uri,
        data_base64: asset.base64!,
        mime: asset.mimeType ?? 'image/jpeg',
        name: asset.fileName ?? undefined,
      },
    ]);
  };

  const handleSend = async (mode: 'steer' | 'queue' = 'steer') => {
    const trimmed = text.trim();
    if ((!trimmed && images.length === 0) || sending) return;
    if (trimmed === '/files' && onLocalCommand) {
      setText('');
      onLocalCommand('files');
      return;
    }
    if (trimmed === '/model') {
      setText('');
      setShowModelPicker(true);
      return;
    }
    setSending(true);
    try {
      await onSend(
        trimmed,
        images.map(({ data_base64, mime, name }) => ({ data_base64, mime, name })),
        mode,
      );
      setText('');
      setImages([]);
      setSelection({ start: 0, end: 0 });
    } finally {
      setSending(false);
    }
  };

  const permLabel =
    permissionModes.find((m) => m.key === permissionMode)?.label ?? permissionMode ?? 'Auto';

  return (
    <View style={[styles.wrap, { backgroundColor: colors.surface, borderTopColor: colors.border }]}>
      {completionVisible && token ? (
        <View
          style={[
            styles.popup,
            { backgroundColor: colors.surfaceElevated, borderColor: colors.border },
          ]}
        >
          <View style={[styles.popupHeader, { borderBottomColor: colors.border }]}>
            <Text style={[styles.popupTitle, { color: colors.muted }]}>
              {token.kind === 'command' ? 'Slash commands (/)' : 'Workspace files (@)'}
            </Text>
            <Pressable onPress={() => setDismissedAt(tokenKey)}>
              <Ionicons name="close" size={16} color={colors.dim} />
            </Pressable>
          </View>
          <ScrollView style={styles.popupList} keyboardShouldPersistTaps="handled">
            {token.kind === 'command' ? (
              commandChoices.length === 0 ? (
                <Text style={[styles.emptyPopup, { color: colors.dim }]}>No matching commands</Text>
              ) : (
                commandChoices.slice(0, 12).map((cmd) => (
                  <Pressable
                    key={cmd.name}
                    onPress={() => pickCompletion(cmd.name)}
                    style={[styles.popupRow, { borderBottomColor: colors.border }]}
                  >
                    <Text style={[styles.cmdName, { color: colors.accent }]}>/{cmd.name}</Text>
                    <Text numberOfLines={1} style={[styles.cmdDesc, { color: colors.muted }]}>
                      {cmd.description}
                    </Text>
                  </Pressable>
                ))
              )
            ) : fileChoices.length === 0 ? (
              <Text style={[styles.emptyPopup, { color: colors.dim }]}>
                {token.query ? 'No matching files' : 'Type a filename after @'}
              </Text>
            ) : (
              fileChoices.slice(0, 15).map((file) => (
                <Pressable
                  key={file.path}
                  onPress={() => pickCompletion(file.path)}
                  style={[styles.popupRow, { borderBottomColor: colors.border }]}
                >
                  <Ionicons name="document-outline" size={13} color={colors.accent} />
                  <Text numberOfLines={1} style={[styles.filePath, { color: colors.text }]}>
                    {file.path}
                  </Text>
                </Pressable>
              ))
            )}
          </ScrollView>
        </View>
      ) : null}

      {images.length > 0 ? (
        <ScrollView horizontal style={styles.draftImages} contentContainerStyle={{ gap: 8 }}>
          {images.map((img, idx) => (
            <View key={idx} style={styles.draftThumbWrap}>
              <Image source={{ uri: img.uri }} style={styles.draftThumb} />
              <Pressable
                onPress={() => setImages((prev) => prev.filter((_, i) => i !== idx))}
                style={styles.removeThumb}
              >
                <Ionicons name="close-circle" size={18} color="#fff" />
              </Pressable>
            </View>
          ))}
        </ScrollView>
      ) : null}

      <View style={styles.pillRow}>
        <Pressable
          onPress={() => setShowPermPicker(true)}
          style={[styles.chip, { backgroundColor: colors.surfaceElevated, borderColor: colors.border }]}
        >
          <Ionicons name="shield-outline" size={12} color={colors.accent} />
          <Text style={[styles.chipText, { color: colors.text }]}>{permLabel}</Text>
        </Pressable>
        <Pressable
          onPress={() => setShowModelPicker(true)}
          style={[styles.chip, { backgroundColor: colors.surfaceElevated, borderColor: colors.border }]}
        >
          <Ionicons name="hardware-chip-outline" size={12} color={colors.muted} />
          <Text numberOfLines={1} style={[styles.chipText, { color: colors.text, maxWidth: 160 }]}>
            {currentModel || 'Model'}
          </Text>
        </Pressable>
        <Pressable
          onPress={() => {
            const next = text.startsWith('/') ? text : `/${text}`;
            setText(next);
            setSelection({ start: 1, end: 1 });
          }}
          style={[styles.chip, { backgroundColor: colors.surfaceElevated, borderColor: colors.border }]}
        >
          <Text style={[styles.chipText, { color: colors.accent }]}>/</Text>
        </Pressable>
        <Pressable
          onPress={() => {
            const insertAt = selection.start;
            const prefix = insertAt > 0 && !/\s/.test(text[insertAt - 1]) ? ' @' : '@';
            const next = `${text.slice(0, insertAt)}${prefix}${text.slice(insertAt)}`;
            const cur = insertAt + prefix.length;
            setText(next);
            setSelection({ start: cur, end: cur });
          }}
          style={[styles.chip, { backgroundColor: colors.surfaceElevated, borderColor: colors.border }]}
        >
          <Text style={[styles.chipText, { color: colors.accent }]}>@</Text>
        </Pressable>
      </View>

      <View style={styles.inputRow}>
        <Pressable onPress={attachImage} style={styles.iconBtn}>
          <Ionicons name="image-outline" size={22} color={colors.muted} />
        </Pressable>
        <TextInput
          value={text}
          onChangeText={(next) => {
            setText(next);
            setDismissedAt(null);
          }}
          onSelectionChange={(e) => setSelection(e.nativeEvent.selection)}
          placeholder="Message Pi… (/ commands, @ files)"
          placeholderTextColor={colors.dim}
          multiline
          style={[
            styles.input,
            { backgroundColor: colors.codeBg, color: colors.text, borderColor: colors.border },
          ]}
        />
        {busy ? (
          <Pressable
            onPress={() => void onAbort()}
            style={[styles.sendBtn, { backgroundColor: colors.red }]}
          >
            <Ionicons name="stop" size={18} color="#fff" />
          </Pressable>
        ) : null}
        <Pressable
          disabled={sending || (!text.trim() && images.length === 0)}
          onPress={() => void handleSend('steer')}
          style={[
            styles.sendBtn,
            {
              backgroundColor:
                text.trim() || images.length > 0 ? colors.accent : colors.surfaceElevated,
            },
          ]}
        >
          <Ionicons
            name="arrow-up"
            size={18}
            color={text.trim() || images.length > 0 ? '#fff' : colors.dim}
          />
        </Pressable>
      </View>

      <Modal
        visible={showPermPicker}
        transparent
        animationType="fade"
        onRequestClose={() => setShowPermPicker(false)}
      >
        <Pressable style={styles.modalBg} onPress={() => setShowPermPicker(false)}>
          <View style={[styles.sheet, { backgroundColor: colors.surface, borderColor: colors.border }]}>
            <Text style={[styles.sheetTitle, { color: colors.text }]}>Permission mode</Text>
            {permissionModes.map((mode) => (
              <Pressable
                key={mode.key}
                onPress={() => {
                  setShowPermPicker(false);
                  void onChangePermission(mode.key);
                }}
                style={[
                  styles.sheetRow,
                  {
                    backgroundColor:
                      mode.key === permissionMode ? colors.accentBg : 'transparent',
                  },
                ]}
              >
                <Text style={{ color: colors.text, fontSize: 14, fontWeight: '600' }}>
                  {mode.label}
                </Text>
                {mode.key === permissionMode ? (
                  <Ionicons name="checkmark" size={16} color={colors.accent} />
                ) : null}
              </Pressable>
            ))}
          </View>
        </Pressable>
      </Modal>

      <Modal
        visible={showModelPicker}
        transparent
        animationType="fade"
        onRequestClose={() => setShowModelPicker(false)}
      >
        <Pressable style={styles.modalBg} onPress={() => setShowModelPicker(false)}>
          <View
            style={[
              styles.sheet,
              { backgroundColor: colors.surface, borderColor: colors.border, maxHeight: '70%' },
            ]}
          >
            <Text style={[styles.sheetTitle, { color: colors.text }]}>Choose model</Text>
            <FlatList
              data={models}
              keyExtractor={(m) => `${m.provider}/${m.id}`}
              renderItem={({ item }) => (
                <Pressable
                  onPress={() => {
                    setShowModelPicker(false);
                    void onChangeModel(item.provider, item.id);
                  }}
                  style={[
                    styles.sheetRow,
                    {
                      backgroundColor:
                        item.id === currentModel ? colors.accentBg : 'transparent',
                    },
                  ]}
                >
                  <View style={{ flex: 1 }}>
                    <Text style={{ color: colors.text, fontSize: 13.5, fontWeight: '600' }}>
                      {item.name || item.id}
                    </Text>
                    <Text style={{ color: colors.dim, fontSize: 11 }}>{item.provider}</Text>
                  </View>
                  {item.id === currentModel ? (
                    <Ionicons name="checkmark" size={16} color={colors.accent} />
                  ) : null}
                </Pressable>
              )}
              ListEmptyComponent={
                <Text style={{ color: colors.dim, padding: 12 }}>No models reported yet.</Text>
              }
            />
          </View>
        </Pressable>
      </Modal>
    </View>
  );
}

const styles = StyleSheet.create({
  wrap: {
    borderTopWidth: 1,
    paddingHorizontal: 10,
    paddingTop: 6,
    paddingBottom: 10,
  },
  popup: {
    borderWidth: 1,
    borderRadius: 10,
    marginBottom: 8,
    overflow: 'hidden',
  },
  popupHeader: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    paddingHorizontal: 10,
    paddingVertical: 6,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  popupTitle: {
    fontSize: 11,
    fontWeight: '700',
    textTransform: 'uppercase',
  },
  popupList: {
    maxHeight: 190,
  },
  popupRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingHorizontal: 10,
    paddingVertical: 9,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  cmdName: {
    fontFamily: 'monospace',
    fontSize: 13,
    fontWeight: '700',
  },
  cmdDesc: {
    fontSize: 12,
    flex: 1,
  },
  filePath: {
    fontFamily: 'monospace',
    fontSize: 12,
    flex: 1,
  },
  emptyPopup: {
    padding: 10,
    fontSize: 12,
  },
  draftImages: {
    marginBottom: 8,
  },
  draftThumbWrap: {
    position: 'relative',
  },
  draftThumb: {
    width: 56,
    height: 56,
    borderRadius: 8,
  },
  removeThumb: {
    position: 'absolute',
    top: -4,
    right: -4,
  },
  pillRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    marginBottom: 6,
  },
  chip: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 4,
    paddingHorizontal: 8,
    paddingVertical: 4,
    borderRadius: 999,
    borderWidth: StyleSheet.hairlineWidth,
  },
  chipText: {
    fontSize: 11.5,
    fontWeight: '600',
  },
  inputRow: {
    flexDirection: 'row',
    alignItems: 'flex-end',
    gap: 6,
  },
  iconBtn: {
    padding: 8,
    justifyContent: 'center',
    alignItems: 'center',
  },
  input: {
    flex: 1,
    minHeight: 40,
    maxHeight: 120,
    borderWidth: 1,
    borderRadius: 10,
    paddingHorizontal: 11,
    paddingVertical: 9,
    fontSize: 14,
  },
  sendBtn: {
    width: 38,
    height: 38,
    borderRadius: 10,
    justifyContent: 'center',
    alignItems: 'center',
  },
  modalBg: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.55)',
    justifyContent: 'flex-end',
    padding: 12,
  },
  sheet: {
    borderWidth: 1,
    borderRadius: 14,
    padding: 12,
  },
  sheetTitle: {
    fontSize: 15,
    fontWeight: '700',
    marginBottom: 8,
  },
  sheetRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 10,
    paddingVertical: 10,
    borderRadius: 8,
  },
});
