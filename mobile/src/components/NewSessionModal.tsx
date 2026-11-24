import React, { useEffect, useState } from 'react';
import { Modal, Pressable, ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import type { PermissionModeOption, SavedHost } from '../protocol/types';
import { usePalette } from '../theme/colors';

interface Props {
  visible: boolean;
  hosts?: SavedHost[];
  activeHostId?: string | null;
  onSelectHost?: (hostId: string) => Promise<void>;
  projects: string[];
  permissionModes: PermissionModeOption[];
  deviceLabel: string;
  onClose: () => void;
  onCreate: (input: { cwd: string; permissionMode: string; prompt?: string }) => Promise<void>;
}

export function NewSessionModal({
  visible,
  hosts = [],
  activeHostId,
  onSelectHost,
  projects,
  permissionModes,
  deviceLabel,
  onClose,
  onCreate,
}: Props) {
  const colors = usePalette();
  const [selectedCwd, setSelectedCwd] = useState(projects[0] ?? '');
  const [customCwd, setCustomCwd] = useState('');
  const [permissionMode, setPermissionMode] = useState(permissionModes[0]?.key ?? 'auto');
  const [prompt, setPrompt] = useState('');
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (projects.length > 0 && !projects.includes(selectedCwd)) {
      setSelectedCwd(projects[0]);
    }
  }, [projects, selectedCwd]);

  const effectiveCwd = customCwd.trim() || selectedCwd || projects[0] || '';

  const submit = async () => {
    if (!effectiveCwd || submitting) return;
    setSubmitting(true);
    try {
      await onCreate({
        cwd: effectiveCwd,
        permissionMode,
        prompt: prompt.trim() || undefined,
      });
      setPrompt('');
      setCustomCwd('');
      onClose();
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Modal visible={visible} transparent animationType="slide" onRequestClose={onClose}>
      <View style={styles.backdrop}>
        <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
          <View style={styles.header}>
            <Text style={[styles.title, { color: colors.text }]}>New session</Text>
            <Pressable onPress={onClose}>
              <Ionicons name="close" size={22} color={colors.muted} />
            </Pressable>
          </View>
          <Text style={[styles.originNote, { color: colors.accent }]}>
            Origin will be tagged as: {deviceLabel}
          </Text>

          <ScrollView style={{ maxHeight: 440 }}>
            {hosts.length > 1 && onSelectHost ? (
              <View style={{ marginBottom: 12 }}>
                <Text style={[styles.label, { color: colors.muted }]}>Where to run</Text>
                <View style={styles.hostGrid}>
                  {hosts.map((h) => {
                    const active = h.id === activeHostId;
                    const isCloud = h.name.toLowerCase().includes('cloud');
                    return (
                      <Pressable
                        key={h.id}
                        onPress={() => void onSelectHost(h.id)}
                        style={[
                          styles.hostPill,
                          {
                            backgroundColor: active ? colors.accentBg : colors.surfaceElevated,
                            borderColor: active ? colors.accent : colors.border,
                          },
                        ]}
                      >
                        <Ionicons
                          name={isCloud ? 'cloud-outline' : 'laptop-outline'}
                          size={14}
                          color={active ? colors.accent : colors.muted}
                        />
                        <Text
                          numberOfLines={1}
                          style={{
                            color: active ? colors.accent : colors.text,
                            fontSize: 12.5,
                            fontWeight: '600',
                          }}
                        >
                          {h.name}
                        </Text>
                      </Pressable>
                    );
                  })}
                </View>
              </View>
            ) : null}

            <Text style={[styles.label, { color: colors.muted }]}>Project folder</Text>
            <View style={styles.list}>
              {projects.map((p) => {
                const active = effectiveCwd === p && !customCwd.trim();
                const short = p.split('/').filter(Boolean).pop() ?? p;
                return (
                  <Pressable
                    key={p}
                    onPress={() => {
                      setSelectedCwd(p);
                      setCustomCwd('');
                    }}
                    style={[
                      styles.projectRow,
                      {
                        backgroundColor: active ? colors.accentBg : colors.surfaceElevated,
                        borderColor: active ? colors.accent : colors.border,
                      },
                    ]}
                  >
                    <Ionicons name="folder-outline" size={15} color={colors.accent} />
                    <View style={{ flex: 1 }}>
                      <Text style={{ color: colors.text, fontSize: 13, fontWeight: '600' }}>
                        {short}
                      </Text>
                      <Text numberOfLines={1} style={{ color: colors.dim, fontSize: 11 }}>
                        {p}
                      </Text>
                    </View>
                  </Pressable>
                );
              })}
            </View>

            <TextInput
              value={customCwd}
              onChangeText={setCustomCwd}
              placeholder="Or enter another host folder path…"
              placeholderTextColor={colors.dim}
              style={[
                styles.input,
                { backgroundColor: colors.codeBg, color: colors.text, borderColor: colors.border },
              ]}
            />

            <Text style={[styles.label, { color: colors.muted, marginTop: 12 }]}>
              Permission mode
            </Text>
            <View style={styles.modeGrid}>
              {permissionModes.map((m) => {
                const active = m.key === permissionMode;
                return (
                  <Pressable
                    key={m.key}
                    onPress={() => setPermissionMode(m.key)}
                    style={[
                      styles.modePill,
                      {
                        backgroundColor: active ? colors.accentBg : colors.surfaceElevated,
                        borderColor: active ? colors.accent : colors.border,
                      },
                    ]}
                  >
                    <Text
                      style={{
                        color: active ? colors.accent : colors.text,
                        fontSize: 12,
                        fontWeight: '600',
                      }}
                    >
                      {m.label}
                    </Text>
                  </Pressable>
                );
              })}
            </View>

            <Text style={[styles.label, { color: colors.muted, marginTop: 12 }]}>
              First message (optional)
            </Text>
            <TextInput
              value={prompt}
              onChangeText={setPrompt}
              placeholder="What should Pi work on?"
              placeholderTextColor={colors.dim}
              multiline
              style={[
                styles.input,
                {
                  minHeight: 72,
                  backgroundColor: colors.codeBg,
                  color: colors.text,
                  borderColor: colors.border,
                },
              ]}
            />
          </ScrollView>

          <Pressable
            disabled={!effectiveCwd || submitting}
            onPress={() => void submit()}
            style={[
              styles.createBtn,
              { backgroundColor: effectiveCwd ? colors.accent : colors.surfaceElevated },
            ]}
          >
            <Text style={{ color: '#fff', fontSize: 14, fontWeight: '700' }}>
              {submitting ? 'Starting…' : 'Start session on host'}
            </Text>
          </Pressable>
        </View>
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  backdrop: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.6)',
    justifyContent: 'flex-end',
  },
  card: {
    borderTopLeftRadius: 18,
    borderTopRightRadius: 18,
    borderWidth: 1,
    padding: 16,
  },
  header: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: 4,
  },
  title: {
    fontSize: 17,
    fontWeight: '700',
  },
  originNote: {
    fontSize: 11.5,
    fontWeight: '600',
    marginBottom: 12,
  },
  label: {
    fontSize: 12,
    fontWeight: '600',
    marginBottom: 6,
  },
  list: {
    gap: 6,
    marginBottom: 8,
  },
  hostGrid: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    gap: 8,
  },
  hostPill: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    borderWidth: 1,
    borderRadius: 999,
    paddingHorizontal: 12,
    paddingVertical: 7,
  },
  projectRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    padding: 10,
    borderRadius: 9,
    borderWidth: 1,
  },
  input: {
    borderWidth: 1,
    borderRadius: 9,
    paddingHorizontal: 10,
    paddingVertical: 8,
    fontSize: 13,
  },
  modeGrid: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    gap: 6,
  },
  modePill: {
    paddingHorizontal: 10,
    paddingVertical: 6,
    borderRadius: 999,
    borderWidth: 1,
  },
  createBtn: {
    marginTop: 14,
    paddingVertical: 12,
    borderRadius: 10,
    alignItems: 'center',
  },
});
