import React, { useEffect, useMemo, useState } from 'react';
import {
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
import { isRtlText } from '../utils/markdown';
import { formatLevelLabel, ModelEffortSheet } from './ModelEffortSheet';

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
  currentModelId: string;
  currentModelLabel: string;
  currentThinking: string;
  thinkingLevels: string[];
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
  onChangeThinking: (level: string) => Promise<void>;
  onLocalCommand?: (name: string) => void;
}

export function Composer({
  cwd,
  busy,
  permissionMode,
  permissionModes,
  currentModelId,
  currentModelLabel,
  currentThinking,
  thinkingLevels,
  models,
  slashCommands,
  onFetchFileMentions,
  onSend,
  onAbort,
  onChangePermission,
  onChangeModel,
  onChangeThinking,
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
  const [showAttachMenu, setShowAttachMenu] = useState(false);

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

  /** Multi-select image upload (up to 10 photos at once). */
  const attachImages = async () => {
    setShowAttachMenu(false);
    const res = await ImagePicker.launchImageLibraryAsync({
      mediaTypes: ['images'],
      allowsMultipleSelection: true,
      selectionLimit: 10,
      base64: true,
      quality: 0.85,
    });
    if (res.canceled || !res.assets?.length) return;
    const picked: DraftImage[] = [];
    for (const asset of res.assets) {
      if (!asset.base64) continue;
      picked.push({
        uri: asset.uri,
        data_base64: asset.base64,
        mime: asset.mimeType ?? 'image/jpeg',
        name: asset.fileName ?? undefined,
      });
    }
    if (picked.length > 0) {
      setImages((prev) => [...prev, ...picked].slice(0, 12));
    }
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
  const effortShort = currentThinking ? formatLevelLabel(currentThinking) : '';
  const modelPillLabel = currentModelLabel
    ? effortShort
      ? `${effortShort} · ${currentModelLabel}`
      : currentModelLabel
    : 'Model';
  const canSend = Boolean(text.trim() || images.length > 0);
  const inputRtl = isRtlText(text);

  return (
    <View style={[styles.outer, { backgroundColor: colors.bg }]}>
      {completionVisible && token ? (
        <View
          style={[
            styles.popup,
            { backgroundColor: colors.surface, borderColor: colors.border },
          ]}
        >
          <View style={[styles.popupHeader, { borderBottomColor: colors.hairline }]}>
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
                    style={[styles.popupRow, { borderBottomColor: colors.hairline }]}
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
                  style={[styles.popupRow, { borderBottomColor: colors.hairline }]}
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

      <View
        style={[
          styles.card,
          { backgroundColor: colors.surface, borderColor: colors.border },
        ]}
      >
        {images.length > 0 ? (
          <ScrollView
            horizontal
            showsHorizontalScrollIndicator={false}
            style={styles.draftImages}
            contentContainerStyle={{ gap: 8 }}
          >
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
            {
              color: colors.text,
              textAlign: inputRtl ? 'right' : 'left',
              writingDirection: inputRtl ? 'rtl' : 'ltr',
            },
          ]}
        />

        <View style={styles.actionBar}>
          <View style={styles.leftActions}>
            <Pressable
              onPress={() => setShowAttachMenu(true)}
              style={[
                styles.circleAction,
                { backgroundColor: colors.surfaceElevated, borderColor: colors.border },
              ]}
            >
              <Ionicons name="add" size={18} color={colors.text} />
            </Pressable>

            <Pressable
              onPress={() => setShowModelPicker(true)}
              style={[
                styles.pill,
                { backgroundColor: colors.surfaceElevated, borderColor: colors.border },
              ]}
            >
              <Text
                numberOfLines={1}
                style={[styles.pillText, { color: colors.text, maxWidth: 150 }]}
              >
                {modelPillLabel}
              </Text>
              <Ionicons name="chevron-down" size={12} color={colors.dim} />
            </Pressable>

            <Pressable
              onPress={() => setShowPermPicker(true)}
              style={[
                styles.pill,
                { backgroundColor: colors.surfaceElevated, borderColor: colors.border },
              ]}
            >
              <Text style={[styles.pillText, { color: colors.muted }]}>{permLabel}</Text>
            </Pressable>
          </View>

          <View style={styles.rightActions}>
            {busy ? (
              <Pressable
                onPress={() => void onAbort()}
                style={[styles.sendCircle, { backgroundColor: colors.red }]}
              >
                <Ionicons name="stop" size={16} color="#fff" />
              </Pressable>
            ) : null}
            <Pressable
              disabled={sending || !canSend}
              onPress={() => void handleSend('steer')}
              style={[
                styles.sendCircle,
                {
                  backgroundColor: canSend ? colors.accent : colors.surfaceElevated,
                },
              ]}
            >
              <Ionicons
                name="arrow-up"
                size={18}
                color={canSend ? '#fff' : colors.dim}
              />
            </Pressable>
          </View>
        </View>
      </View>

      {/* Attach / Quick insert sheet (+ button) */}
      <Modal
        visible={showAttachMenu}
        transparent
        animationType="fade"
        onRequestClose={() => setShowAttachMenu(false)}
      >
        <Pressable style={styles.modalBg} onPress={() => setShowAttachMenu(false)}>
          <View
            style={[
              styles.sheet,
              { backgroundColor: colors.surface, borderColor: colors.border },
            ]}
          >
            <Text style={[styles.sheetTitle, { color: colors.text }]}>Add to message</Text>
            <Pressable
              onPress={() => void attachImages()}
              style={[styles.sheetRow, { borderBottomColor: colors.hairline }]}
            >
              <View style={styles.sheetRowLeft}>
                <Ionicons name="images-outline" size={19} color={colors.accent} />
                <View>
                  <Text style={{ color: colors.text, fontSize: 14.5, fontWeight: '600' }}>
                    Upload photos (multi-select)
                  </Text>
                  <Text style={{ color: colors.muted, fontSize: 11.5 }}>
                    Pick up to 10 screenshots or images at once
                  </Text>
                </View>
              </View>
            </Pressable>
            <Pressable
              onPress={() => {
                setShowAttachMenu(false);
                const next = text.startsWith('/') ? text : `/${text}`;
                setText(next);
                setSelection({ start: 1, end: 1 });
              }}
              style={[styles.sheetRow, { borderBottomColor: colors.hairline }]}
            >
              <View style={styles.sheetRowLeft}>
                <Ionicons name="terminal-outline" size={19} color={colors.accent} />
                <Text style={{ color: colors.text, fontSize: 14.5, fontWeight: '600' }}>
                  Insert slash command (/)
                </Text>
              </View>
            </Pressable>
            <Pressable
              onPress={() => {
                setShowAttachMenu(false);
                const insertAt = selection.start;
                const prefix = insertAt > 0 && !/\s/.test(text[insertAt - 1]) ? ' @' : '@';
                const next = `${text.slice(0, insertAt)}${prefix}${text.slice(insertAt)}`;
                const cur = insertAt + prefix.length;
                setText(next);
                setSelection({ start: cur, end: cur });
              }}
              style={styles.sheetRow}
            >
              <View style={styles.sheetRowLeft}>
                <Ionicons name="at-outline" size={19} color={colors.accent} />
                <Text style={{ color: colors.text, fontSize: 14.5, fontWeight: '600' }}>
                  Mention workspace file (@)
                </Text>
              </View>
            </Pressable>
          </View>
        </Pressable>
      </Modal>

      {/* Permission picker */}
      <Modal
        visible={showPermPicker}
        transparent
        animationType="fade"
        onRequestClose={() => setShowPermPicker(false)}
      >
        <Pressable style={styles.modalBg} onPress={() => setShowPermPicker(false)}>
          <View
            style={[
              styles.sheet,
              { backgroundColor: colors.surface, borderColor: colors.border },
            ]}
          >
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
                    borderRadius: 10,
                  },
                ]}
              >
                <Text style={{ color: colors.text, fontSize: 14.5, fontWeight: '600' }}>
                  {mode.label}
                </Text>
                {mode.key === permissionMode ? (
                  <Ionicons name="checkmark" size={17} color={colors.accent} />
                ) : null}
              </Pressable>
            ))}
          </View>
        </Pressable>
      </Modal>

      {/* Multi-provider Model & Effort picker */}
      <ModelEffortSheet
        visible={showModelPicker}
        models={models}
        currentModelId={currentModelId}
        currentThinking={currentThinking}
        thinkingLevels={thinkingLevels}
        onClose={() => setShowModelPicker(false)}
        onSelectModel={onChangeModel}
        onSelectThinking={onChangeThinking}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  outer: {
    paddingHorizontal: 10,
    paddingTop: 6,
    paddingBottom: 10,
    direction: 'ltr',
  },
  card: {
    borderWidth: 1,
    borderRadius: 22,
    paddingHorizontal: 14,
    paddingTop: 10,
    paddingBottom: 9,
    gap: 8,
  },
  input: {
    minHeight: 36,
    maxHeight: 120,
    fontSize: 15,
    lineHeight: 21,
    paddingVertical: 2,
  },
  actionBar: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 8,
  },
  leftActions: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    flex: 1,
    flexWrap: 'wrap',
  },
  rightActions: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  circleAction: {
    width: 32,
    height: 32,
    borderRadius: 16,
    borderWidth: StyleSheet.hairlineWidth,
    alignItems: 'center',
    justifyContent: 'center',
  },
  pill: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 4,
    paddingHorizontal: 10,
    height: 32,
    borderRadius: 16,
    borderWidth: StyleSheet.hairlineWidth,
  },
  pillText: {
    fontSize: 12,
    fontWeight: '600',
  },
  sendCircle: {
    width: 34,
    height: 34,
    borderRadius: 17,
    alignItems: 'center',
    justifyContent: 'center',
  },
  popup: {
    borderWidth: 1,
    borderRadius: 14,
    marginBottom: 8,
    overflow: 'hidden',
  },
  popupHeader: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    paddingHorizontal: 12,
    paddingVertical: 7,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  popupTitle: {
    fontSize: 11,
    fontWeight: '700',
    textTransform: 'uppercase',
  },
  popupList: {
    maxHeight: 200,
  },
  popupRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingHorizontal: 12,
    paddingVertical: 10,
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
    padding: 12,
    fontSize: 12,
  },
  draftImages: {
    marginBottom: 2,
  },
  draftThumbWrap: {
    position: 'relative',
  },
  draftThumb: {
    width: 58,
    height: 58,
    borderRadius: 10,
  },
  removeThumb: {
    position: 'absolute',
    top: -4,
    right: -4,
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
    fontSize: 15.5,
    fontWeight: '700',
    marginBottom: 8,
  },
  sheetRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 10,
    paddingVertical: 12,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: 'transparent',
  },
  sheetRowLeft: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    flex: 1,
  },
});
