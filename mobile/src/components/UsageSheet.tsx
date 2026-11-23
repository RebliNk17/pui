import React from 'react';
import { Modal, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import type { SessionDetail } from '../protocol/types';
import { usePalette } from '../theme/colors';
import { OriginBadge } from './OriginBadge';

interface Props {
  visible: boolean;
  detail: SessionDetail | undefined;
  onClose: () => void;
  onRefresh: () => void;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(Math.round(n));
}

export function UsageSheet({ visible, detail, onClose, onRefresh }: Props) {
  const colors = usePalette();
  const tokens = detail?.context_tokens ?? 0;
  const window = detail?.context_window ?? 0;
  const pct = window > 0 ? Math.min(100, Math.round((tokens / window) * 100)) : 0;

  return (
    <Modal visible={visible} transparent animationType="slide" onRequestClose={onClose}>
      <Pressable style={styles.backdrop} onPress={onClose}>
        <Pressable
          onPress={(e) => e.stopPropagation()}
          style={[styles.sheet, { backgroundColor: colors.bg, borderColor: colors.border }]}
        >
          <View style={[styles.handle, { backgroundColor: colors.border }]} />
          <View style={styles.header}>
            <Pressable onPress={onRefresh} style={styles.iconBtn}>
              <Ionicons name="refresh" size={19} color={colors.muted} />
            </Pressable>
            <Text style={[styles.title, { color: colors.text }]}>Usage &amp; Context</Text>
            <Pressable onPress={onClose} style={styles.iconBtn}>
              <Ionicons name="close" size={21} color={colors.muted} />
            </Pressable>
          </View>

          <ScrollView contentContainerStyle={styles.body}>
            <Text style={[styles.section, { color: colors.muted }]}>Context</Text>
            <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
              <View style={styles.rowBetween}>
                <Text style={[styles.cardTitle, { color: colors.text }]}>Context window</Text>
                <Text style={[styles.pctText, { color: colors.muted }]}>{pct}% used</Text>
              </View>
              <View style={[styles.track, { backgroundColor: colors.surfaceElevated }]}>
                <View
                  style={[
                    styles.fill,
                    {
                      width: `${Math.max(pct, pct > 0 ? 3 : 0)}%`,
                      backgroundColor:
                        pct >= 85 ? colors.red : pct >= 65 ? colors.amber : colors.accent,
                    },
                  ]}
                />
              </View>
              <Text style={[styles.subText, { color: colors.muted }]}>
                {window > 0
                  ? `${formatTokens(tokens)} of ${formatTokens(window)} tokens`
                  : 'Waiting for token metrics from Pi'}
              </Text>
            </View>

            <Text style={[styles.section, { color: colors.muted }]}>Session</Text>
            <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
              <View style={styles.metaRow}>
                <Text style={{ color: colors.muted, fontSize: 13 }}>Origin</Text>
                <OriginBadge origin={detail?.origin} />
              </View>
              <View style={[styles.divider, { backgroundColor: colors.hairline }]} />
              <View style={styles.metaRow}>
                <Text style={{ color: colors.muted, fontSize: 13 }}>Model</Text>
                <Text numberOfLines={1} style={{ color: colors.text, fontSize: 13, fontWeight: '600', maxWidth: 210 }}>
                  {detail?.model_name || detail?.model || '—'}
                </Text>
              </View>
              <View style={[styles.divider, { backgroundColor: colors.hairline }]} />
              <View style={styles.metaRow}>
                <Text style={{ color: colors.muted, fontSize: 13 }}>Thinking effort</Text>
                <Text style={{ color: colors.text, fontSize: 13, fontWeight: '600' }}>
                  {detail?.thinking || 'default'}
                </Text>
              </View>
              <View style={[styles.divider, { backgroundColor: colors.hairline }]} />
              <View style={styles.metaRow}>
                <Text style={{ color: colors.muted, fontSize: 13 }}>Permission mode</Text>
                <Text style={{ color: colors.text, fontSize: 13, fontWeight: '600' }}>
                  {detail?.permission_mode || 'auto'}
                </Text>
              </View>
              <View style={[styles.divider, { backgroundColor: colors.hairline }]} />
              <View style={styles.metaRow}>
                <Text style={{ color: colors.muted, fontSize: 13 }}>Session cost</Text>
                <Text style={{ color: colors.text, fontSize: 13, fontWeight: '600' }}>
                  ${(detail?.cost ?? 0).toFixed(4)}
                </Text>
              </View>
            </View>
          </ScrollView>
        </Pressable>
      </Pressable>
    </Modal>
  );
}

const styles = StyleSheet.create({
  backdrop: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.62)',
    justifyContent: 'flex-end',
  },
  sheet: {
    borderTopLeftRadius: 22,
    borderTopRightRadius: 22,
    borderTopWidth: 1,
    paddingBottom: 28,
    maxHeight: '82%',
    direction: 'ltr',
  },
  handle: {
    width: 38,
    height: 4,
    borderRadius: 2,
    alignSelf: 'center',
    marginTop: 10,
    marginBottom: 6,
  },
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 14,
    paddingVertical: 8,
  },
  iconBtn: {
    width: 34,
    height: 34,
    alignItems: 'center',
    justifyContent: 'center',
  },
  title: {
    fontSize: 16,
    fontWeight: '700',
  },
  body: {
    paddingHorizontal: 16,
    paddingTop: 6,
    gap: 10,
  },
  section: {
    fontSize: 12.5,
    fontWeight: '600',
    marginTop: 6,
    marginLeft: 2,
  },
  card: {
    borderRadius: 14,
    borderWidth: 1,
    padding: 14,
    gap: 10,
  },
  rowBetween: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
  },
  cardTitle: {
    fontSize: 15,
    fontWeight: '600',
  },
  pctText: {
    fontSize: 13,
  },
  track: {
    height: 8,
    borderRadius: 4,
    overflow: 'hidden',
  },
  fill: {
    height: 8,
    borderRadius: 4,
  },
  subText: {
    fontSize: 12.5,
  },
  metaRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    paddingVertical: 2,
  },
  divider: {
    height: StyleSheet.hairlineWidth,
  },
});
