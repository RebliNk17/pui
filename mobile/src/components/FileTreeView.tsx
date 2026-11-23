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
import { HtmlArtifactCard } from './HtmlArtifactCard';

interface Props {
  cwd: string;
  transport: PuiTransport | null;
  initialFilePath?: string | null;
  onOpenImage: (uri: string, title?: string) => void;
}

export function FileTreeView({ cwd, transport, initialFilePath, onOpenImage }: Props) {
  const colors = usePalette();
  const [dir, setDir] = useState('');
  const [entries, setEntries] = useState<TreeEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedFile, setSelectedFile] = useState<FileContentResponse | null>(null);

  const loadDir = useCallback(
    async (nextDir: string) => {
      if (!transport || !cwd) return;
      setLoading(true);
      setError(null);
      try {
        const res = await transport.getFileTree(cwd, nextDir);
        setDir(res.dir);
        setEntries(res.entries);
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : 'Failed to load files');
      } finally {
        setLoading(false);
      }
    },
    [transport, cwd],
  );

  const openFile = useCallback(
    async (filePath: string) => {
      if (!transport || !cwd) return;
      setLoading(true);
      setError(null);
      try {
        const content = await transport.getFileContent(cwd, filePath);
        setSelectedFile(content);
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : 'Failed to read file');
      } finally {
        setLoading(false);
      }
    },
    [transport, cwd],
  );

  useEffect(() => {
    void loadDir('');
  }, [loadDir]);

  useEffect(() => {
    if (initialFilePath) {
      void openFile(initialFilePath);
    }
  }, [initialFilePath, openFile]);

  if (selectedFile) {
    const rawUrl = transport ? transport.getRawFileUrl(cwd, selectedFile.path) : '';
    return (
      <View style={styles.flex}>
        <View style={[styles.bar, { backgroundColor: colors.surface, borderBottomColor: colors.border }]}>
          <Pressable onPress={() => setSelectedFile(null)} style={styles.backBtn}>
            <Ionicons name="arrow-back" size={18} color={colors.accent} />
            <Text style={{ color: colors.accent, fontSize: 13, fontWeight: '600' }}>Files</Text>
          </Pressable>
          <Text numberOfLines={1} style={[styles.pathLabel, { color: colors.text }]}>
            {selectedFile.path}
          </Text>
        </View>
        <ScrollView style={styles.flex} contentContainerStyle={{ padding: 12 }}>
          {selectedFile.is_image && rawUrl ? (
            <Pressable onPress={() => onOpenImage(rawUrl, selectedFile.path)}>
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
              title={selectedFile.path}
              html={selectedFile.content}
              initialHeight={380}
            />
          ) : selectedFile.is_binary ? (
            <Text style={{ color: colors.muted, fontSize: 13 }}>
              Binary file ({selectedFile.mime}, {formatBytes(selectedFile.size)}).
            </Text>
          ) : (
            <View style={[styles.codeBox, { backgroundColor: colors.codeBg, borderColor: colors.border }]}>
              <Text selectable style={[styles.code, { color: colors.text }]}>
                {selectedFile.content ?? ''}
              </Text>
            </View>
          )}
        </ScrollView>
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

      {error ? (
        <Text style={{ color: colors.red, padding: 14, fontSize: 13 }}>{error}</Text>
      ) : loading ? (
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
});
