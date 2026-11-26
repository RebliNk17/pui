import React, { useCallback, useEffect, useState } from 'react';
import {
  ActivityIndicator,
  FlatList,
  Image,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import type { FileContentResponse, TreeEntry } from '../protocol/types';
import type { PuiTransport } from '../transport/transport';
import { usePalette } from '../theme/colors';
import { isMarkdownPath, resolveMarkdownAssetPath } from '../utils/markdown';
import { CustomScrollbarOverlay, useCustomScrollbar } from './CustomScrollbar';
import { HtmlArtifactCard } from './HtmlArtifactCard';
import { MarkdownView } from './MessageBubble';

interface Props {
  cwd: string;
  transport: PuiTransport | null;
  initialFilePath?: string | null;
  onOpenImage: (uri: string, title?: string) => void;
  onCloseDirectFile?: () => void;
}

export function FileTreeView({
  cwd,
  transport,
  initialFilePath,
  onOpenImage,
  onCloseDirectFile,
}: Props) {
  const colors = usePalette();
  const [dir, setDir] = useState('');
  const [entries, setEntries] = useState<TreeEntry[]>([]);
  const [treeLoaded, setTreeLoaded] = useState(false);
  const [treeLoading, setTreeLoading] = useState(false);
  const [treeError, setTreeError] = useState<string | null>(null);
  const [fileLoading, setFileLoading] = useState(false);
  const [fileError, setFileError] = useState<string | null>(null);
  const [pendingFilePath, setPendingFilePath] = useState<string | null>(initialFilePath ?? null);
  const [selectedFile, setSelectedFile] = useState<FileContentResponse | null>(null);
  const [mdMode, setMdMode] = useState<'preview' | 'source'>('preview');
  const { bindings, metrics } = useCustomScrollbar();

  const loadDir = useCallback(
    async (nextDir: string) => {
      if (!transport || !cwd) return;
      setTreeLoading(true);
      setTreeError(null);
      try {
        const res = await transport.getFileTree(cwd, nextDir);
        setDir(res.dir);
        setEntries(res.entries);
        setTreeLoaded(true);
      } catch (e: unknown) {
        setTreeError(e instanceof Error ? e.message : 'Failed to load files');
      } finally {
        setTreeLoading(false);
      }
    },
    [transport, cwd],
  );

  const openFile = useCallback(
    async (filePath: string) => {
      setPendingFilePath(filePath);
      if (!transport || !cwd) {
        setFileError('Not connected to host.');
        return;
      }
      setFileLoading(true);
      setFileError(null);
      try {
        const content = await transport.getFileContent(cwd, filePath);
        setSelectedFile(content);
        setMdMode('preview');
      } catch (e: unknown) {
        setSelectedFile(null);
        setFileError(e instanceof Error ? e.message : 'Failed to read file');
      } finally {
        setFileLoading(false);
      }
    },
    [transport, cwd],
  );

  useEffect(() => {
    if (initialFilePath) {
      void openFile(initialFilePath);
    } else {
      setPendingFilePath(null);
      setSelectedFile(null);
      setFileError(null);
      void loadDir('');
    }
  }, [initialFilePath, openFile, loadDir]);

  const showTree = () => {
    setSelectedFile(null);
    setPendingFilePath(null);
    setFileError(null);
    if (!treeLoaded) {
      void loadDir('');
    }
  };

  // Dedicated loading / error state when opening a specific file (never raced with loadDir).
  if (pendingFilePath && !selectedFile && (fileLoading || fileError)) {
    const displayPending =
      cwd && pendingFilePath.startsWith(`${cwd}/`)
        ? pendingFilePath.slice(cwd.length + 1)
        : pendingFilePath;
    return (
      <View style={styles.flex}>
        <View style={[styles.bar, { backgroundColor: colors.surface, borderBottomColor: colors.border }]}>
          <Pressable
            onPress={() => (onCloseDirectFile ? onCloseDirectFile() : showTree())}
            style={styles.backBtn}
          >
            <Ionicons name="arrow-back" size={18} color={colors.accent} />
            <Text style={{ color: colors.accent, fontSize: 13, fontWeight: '600' }}>
              {onCloseDirectFile ? 'Chat' : 'Files'}
            </Text>
          </Pressable>
          <Text numberOfLines={1} style={[styles.pathLabel, { color: colors.text }]}>
            {displayPending}
          </Text>
        </View>
        {fileLoading ? (
          <View style={styles.centerState}>
            <ActivityIndicator color={colors.accent} />
            <Text style={{ color: colors.muted, fontSize: 13, marginTop: 10 }}>
              Loading file…
            </Text>
          </View>
        ) : (
          <View style={styles.centerState}>
            <Ionicons name="alert-circle-outline" size={28} color={colors.red} />
            <Text style={{ color: colors.red, fontSize: 13.5, textAlign: 'center', marginTop: 8 }}>
              {fileError}
            </Text>
            <View style={{ flexDirection: 'row', gap: 10, marginTop: 14 }}>
              <Pressable
                onPress={() => void openFile(pendingFilePath)}
                style={[styles.actionBtn, { backgroundColor: colors.accentBg }]}
              >
                <Text style={{ color: colors.accent, fontSize: 13, fontWeight: '600' }}>Retry</Text>
              </Pressable>
              <Pressable
                onPress={showTree}
                style={[styles.actionBtn, { backgroundColor: colors.surface, borderColor: colors.border, borderWidth: StyleSheet.hairlineWidth }]}
              >
                <Text style={{ color: colors.text, fontSize: 13, fontWeight: '500' }}>
                  Browse files
                </Text>
              </Pressable>
            </View>
          </View>
        )}
      </View>
    );
  }

  if (selectedFile) {
    const rawUrl = transport ? transport.getRawFileUrl(cwd, selectedFile.path) : '';
    const isMarkdown = !selectedFile.is_binary && isMarkdownPath(selectedFile.path);
    const displayPath =
      cwd && selectedFile.path.startsWith(`${cwd}/`)
        ? selectedFile.path.slice(cwd.length + 1)
        : selectedFile.path;

    const resolveMdImage = (src: string): string | null => {
      if (!transport) return null;
      if (/^https?:\/\//i.test(src.trim())) return src.trim();
      const resolved = resolveMarkdownAssetPath(selectedFile.path, src);
      return resolved ? transport.getRawFileUrl(cwd, resolved) : null;
    };

    return (
      <View style={styles.flex}>
        <View style={[styles.bar, { backgroundColor: colors.surface, borderBottomColor: colors.border }]}>
          <Pressable onPress={showTree} style={styles.backBtn}>
            <Ionicons name="arrow-back" size={18} color={colors.accent} />
            <Text style={{ color: colors.accent, fontSize: 13, fontWeight: '600' }}>Files</Text>
          </Pressable>
          <Text numberOfLines={1} style={[styles.pathLabel, { color: colors.text }]}>
            {displayPath}
          </Text>
          {isMarkdown ? (
            <View
              style={[
                styles.modeToggle,
                { backgroundColor: colors.bg, borderColor: colors.border },
              ]}
            >
              {(['preview', 'source'] as const).map((mode) => {
                const active = mdMode === mode;
                return (
                  <Pressable
                    key={mode}
                    onPress={() => setMdMode(mode)}
                    style={[
                      styles.modeBtn,
                      active && { backgroundColor: colors.accentBg },
                    ]}
                  >
                    <Text
                      style={{
                        color: active ? colors.accent : colors.muted,
                        fontSize: 11,
                        fontWeight: '600',
                      }}
                    >
                      {mode === 'preview' ? 'Preview' : 'Source'}
                    </Text>
                  </Pressable>
                );
              })}
            </View>
          ) : null}
        </View>
        <View style={styles.scrollWrap}>
          <ScrollView
            {...bindings}
            style={styles.flex}
            contentContainerStyle={{ padding: isMarkdown && mdMode === 'preview' ? 16 : 12 }}
          >
            {selectedFile.is_image && rawUrl ? (
              <Pressable onPress={() => onOpenImage(rawUrl, displayPath)}>
                <Image
                  source={{ uri: rawUrl }}
                  style={styles.previewImage}
                  resizeMode="contain"
                />
                <Text style={{ color: colors.dim, fontSize: 11.5, textAlign: 'center', marginTop: 6 }}>
                  Tap image to view fullscreen
                </Text>
              </Pressable>
            ) : selectedFile.is_html && selectedFile.content ? (
              <HtmlArtifactCard
                title={displayPath}
                html={selectedFile.content}
                initialHeight={380}
              />
            ) : selectedFile.is_binary ? (
              <Text style={{ color: colors.muted, fontSize: 13 }}>
                Binary file ({selectedFile.mime}, {formatBytes(selectedFile.size)}).
              </Text>
            ) : isMarkdown && mdMode === 'preview' ? (
              <MarkdownView
                text={selectedFile.content ?? ''}
                colors={colors}
                resolveImageUri={resolveMdImage}
                onOpenImage={onOpenImage}
              />
            ) : (
              <View style={[styles.codeBox, { backgroundColor: colors.codeBg, borderColor: colors.border }]}>
                <Text selectable style={[styles.code, { color: colors.text }]}>
                  {selectedFile.content ?? ''}
                </Text>
              </View>
            )}
          </ScrollView>
          <CustomScrollbarOverlay {...metrics} />
        </View>
      </View>
    );
  }

  const goUp = () => {
    if (!dir) return;
    const parts = dir.split('/').filter(Boolean);
    parts.pop();
    void loadDir(parts.join('/'));
  };

  return (
    <View style={styles.flex}>
      <View style={[styles.bar, { backgroundColor: colors.surface, borderBottomColor: colors.border }]}>
        {dir ? (
          <Pressable onPress={goUp} style={styles.backBtn}>
            <Ionicons name="arrow-up" size={16} color={colors.accent} />
            <Text style={{ color: colors.accent, fontSize: 12.5, fontWeight: '600' }}>Up</Text>
          </Pressable>
        ) : null}
        <Text numberOfLines={1} style={[styles.pathLabel, { color: colors.muted }]}>
          /{dir}
        </Text>
        <Pressable onPress={() => void loadDir(dir)}>
          <Ionicons name="refresh" size={16} color={colors.muted} />
        </Pressable>
      </View>

      {treeError ? (
        <Text style={{ color: colors.red, padding: 14, fontSize: 13 }}>{treeError}</Text>
      ) : treeLoading ? (
        <ActivityIndicator style={{ marginTop: 28 }} color={colors.accent} />
      ) : (
        <FlatList
          data={entries}
          keyExtractor={(item) => item.path}
          renderItem={({ item }) => (
            <Pressable
              onPress={() => (item.is_dir ? void loadDir(item.path) : void openFile(item.path))}
              style={[styles.row, { borderBottomColor: colors.border }]}
            >
              <Ionicons
                name={
                  item.is_dir
                    ? 'folder'
                    : item.is_image
                      ? 'image-outline'
                      : item.is_html
                        ? 'code-slash-outline'
                        : 'document-text-outline'
                }
                size={18}
                color={
                  item.is_dir
                    ? colors.accent
                    : item.is_html
                      ? colors.amber
                      : item.is_image
                        ? colors.green
                        : colors.muted
                }
              />
              <Text numberOfLines={1} style={[styles.name, { color: colors.text }]}>
                {item.name}
              </Text>
              {!item.is_dir ? (
                <Text style={[styles.size, { color: colors.dim }]}>{formatBytes(item.size)}</Text>
              ) : (
                <Ionicons name="chevron-forward" size={15} color={colors.dim} />
              )}
            </Pressable>
          )}
          ListEmptyComponent={
            <Text style={{ color: colors.dim, padding: 16 }}>Empty directory.</Text>
          }
        />
      )}
    </View>
  );
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

const styles = StyleSheet.create({
  flex: { flex: 1 },
  scrollWrap: { flex: 1, position: 'relative' },
  modeToggle: {
    flexDirection: 'row',
    borderRadius: 8,
    borderWidth: StyleSheet.hairlineWidth,
    padding: 2,
    gap: 2,
    flexShrink: 0,
  },
  modeBtn: {
    paddingHorizontal: 8,
    paddingVertical: 4,
    borderRadius: 6,
  },
  bar: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingHorizontal: 12,
    paddingVertical: 10,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  backBtn: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 4,
  },
  pathLabel: {
    flex: 1,
    fontFamily: 'monospace',
    fontSize: 12,
  },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    paddingHorizontal: 14,
    paddingVertical: 12,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  name: {
    flex: 1,
    fontSize: 13.5,
  },
  size: {
    fontSize: 11.5,
  },
  codeBox: {
    borderWidth: 1,
    borderRadius: 10,
    padding: 10,
  },
  code: {
    fontFamily: 'monospace',
    fontSize: 11.5,
    lineHeight: 17,
  },
  previewImage: {
    width: '100%',
    height: 360,
    borderRadius: 10,
  },
  centerState: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: 24,
  },
  actionBtn: {
    paddingHorizontal: 14,
    paddingVertical: 8,
    borderRadius: 8,
  },
});
