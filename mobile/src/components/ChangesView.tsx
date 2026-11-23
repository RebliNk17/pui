import React, { useCallback, useEffect, useState } from 'react';
import {
  ActivityIndicator,
  FlatList,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import type { GitChangeItem, GitChangesResponse } from '../protocol/types';
import type { PuiTransport } from '../transport/transport';
import { usePalette } from '../theme/colors';

interface Props {
  cwd: string;
  transport: PuiTransport | null;
}

export function ChangesView({ cwd, transport }: Props) {
  const colors = usePalette();
  const [changes, setChanges] = useState<GitChangesResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedDiff, setSelectedDiff] = useState<{ path: string; diff: string } | null>(null);

  const refresh = useCallback(async () => {
    if (!transport || !cwd) return;
    setLoading(true);
    setError(null);
    try {
      const res = await transport.getGitChanges(cwd);
      setChanges(res);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Failed to load Git changes');
    } finally {
      setLoading(false);
    }
  }, [transport, cwd]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const openDiff = async (item: GitChangeItem) => {
    if (!transport || !cwd) return;
    setLoading(true);
    try {
      const diff = await transport.getGitDiff(cwd, item.path, item.status);
      setSelectedDiff({ path: item.path, diff });
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Failed to load diff');
    } finally {
      setLoading(false);
    }
  };

  if (selectedDiff) {
    const lines = selectedDiff.diff.split('\n');
    return (
      <View style={styles.flex}>
        <View style={[styles.bar, { backgroundColor: colors.surface, borderBottomColor: colors.border }]}>
          <Pressable onPress={() => setSelectedDiff(null)} style={styles.backBtn}>
            <Ionicons name="arrow-back" size={18} color={colors.accent} />
            <Text style={{ color: colors.accent, fontSize: 13, fontWeight: '600' }}>Changes</Text>
          </Pressable>
          <Text numberOfLines={1} style={[styles.branch, { color: colors.text }]}>
            {selectedDiff.path}
          </Text>
        </View>
        <ScrollView style={[styles.flex, { backgroundColor: colors.codeBg }]}>
          <ScrollView horizontal contentContainerStyle={{ padding: 10 }}>
            <View>
              {lines.map((line, i) => {
                const isAdd = line.startsWith('+') && !line.startsWith('+++');
                const isDel = line.startsWith('-') && !line.startsWith('---');
                const isHunk = line.startsWith('@@');
                return (
                  <Text
                    key={i}
                    style={[
                      styles.diffLine,
                      {
                        color: isAdd
                          ? colors.green
                          : isDel
                            ? colors.red
                            : isHunk
                              ? colors.accent
                              : colors.text,
                        backgroundColor: isAdd
                          ? colors.greenBg
                          : isDel
                            ? colors.redBg
                            : 'transparent',
                      },
                    ]}
                  >
                    {line}
                  </Text>
                );
              })}
            </View>
          </ScrollView>
        </ScrollView>
      </View>
    );
  }

  return (
    <View style={styles.flex}>
      <View style={[styles.bar, { backgroundColor: colors.surface, borderBottomColor: colors.border }]}>
        <Ionicons name="git-branch-outline" size={16} color={colors.accent} />
        <Text style={[styles.branch, { color: colors.text }]}>
          {changes?.is_git ? changes.branch || 'HEAD' : 'Not a Git repository'}
        </Text>
        <Pressable onPress={() => void refresh()}>
          <Ionicons name="refresh" size={16} color={colors.muted} />
        </Pressable>
      </View>
      {error ? (
        <Text style={{ color: colors.red, padding: 14 }}>{error}</Text>
      ) : loading ? (
        <ActivityIndicator style={{ marginTop: 28 }} color={colors.accent} />
      ) : (
        <FlatList
          data={changes?.files ?? []}
          keyExtractor={(item) => item.path}
          renderItem={({ item }) => (
            <Pressable
              onPress={() => void openDiff(item)}
              style={[styles.row, { borderBottomColor: colors.border }]}
            >
              <Text style={[styles.status, { color: colors.amber }]}>{item.status.trim() || 'M'}</Text>
              <Text numberOfLines={1} style={[styles.filePath, { color: colors.text }]}>
                {item.path}
              </Text>
              <Text style={{ color: colors.green, fontFamily: 'monospace', fontSize: 11 }}>
                +{item.added}
              </Text>
              <Text style={{ color: colors.red, fontFamily: 'monospace', fontSize: 11 }}>
                -{item.removed}
              </Text>
            </Pressable>
          )}
          ListEmptyComponent={
            <Text style={{ color: colors.dim, padding: 16 }}>Working tree clean.</Text>
          }
        />
      )}
    </View>
  );
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
  backBtn: { flexDirection: 'row', alignItems: 'center', gap: 4 },
  branch: { flex: 1, fontSize: 13, fontWeight: '600' },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingHorizontal: 14,
    paddingVertical: 12,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  status: { fontFamily: 'monospace', fontSize: 11.5, fontWeight: '700', width: 22 },
  filePath: { flex: 1, fontFamily: 'monospace', fontSize: 12.5 },
  diffLine: { fontFamily: 'monospace', fontSize: 11.5, lineHeight: 17, paddingHorizontal: 4 },
});
