import React, { useCallback, useEffect, useState } from 'react';
import {
  ActivityIndicator,
  Modal,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import type { ProviderUsage, SessionDetail, UsageResponse, UsageWindow } from '../protocol/types';
import type { PuiTransport } from '../transport/transport';
import { usePalette, type Palette } from '../theme/colors';
import { CustomScrollbarOverlay, useCustomScrollbar } from './CustomScrollbar';
import { OriginBadge } from './OriginBadge';

interface Props {
  visible: boolean;
  detail?: SessionDetail;
  transport: PuiTransport | null;
  onClose: () => void;
  onRefreshSession?: () => void;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(Math.round(n));
}

function formatResetTime(resetsMs?: number | null): string | null {
  if (!resetsMs || !Number.isFinite(resetsMs) || resetsMs <= 0) return null;
  const now = Date.now();
  const diffMin = Math.round((resetsMs - now) / 60_000);
  if (diffMin <= 0) return 'Resets soon';
  if (diffMin < 60) return `Resets in ${diffMin}m`;
  const hours = Math.floor(diffMin / 60);
  const remMin = diffMin % 60;
  if (hours < 24) {
    return remMin > 0 ? `Resets in ${hours}h ${remMin}m` : `Resets in ${hours}h`;
  }
  const d = new Date(resetsMs);
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  const weekday = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'][d.getDay()];
  return `Resets ${weekday} ${hh}:${mm}`;
}

export function UsageSheet({ visible, detail, transport, onClose, onRefreshSession }: Props) {
  const colors = usePalette();
  const [usage, setUsage] = useState<UsageResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [expandedModels, setExpandedModels] = useState<Record<string, boolean>>({});
  const { bindings, metrics } = useCustomScrollbar();

  const fetchUsage = useCallback(async (): Promise<UsageResponse | null> => {
    if (!transport) return null;
    try {
      const data = await transport.getUsage();
      setUsage(data);
      setErrorMsg(null);
      return data;
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg.includes('404')) {
        setErrorMsg(
          'Your running Pui Desktop instance is from an earlier build. Quit (⌘Q) and reopen Pui Desktop to enable live provider rate limits.',
        );
      } else {
        setErrorMsg(msg);
      }
      return null;
    }
  }, [transport]);

  const triggerRefreshAndPoll = useCallback(async () => {
    if (!transport) return;
    setLoading(true);
    setErrorMsg(null);
    try {
      onRefreshSession?.();
      await transport.refreshUsage();
      // Host runs a background Node helper across providers (~1–4s); poll until populated.
      for (let attempt = 0; attempt < 6; attempt += 1) {
        await new Promise((r) => setTimeout(r, attempt === 0 ? 700 : 1000));
        const latest = await fetchUsage();
        if (latest && (latest.providers.length > 0 || latest.data)) {
          break;
        }
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg.includes('404')) {
        setErrorMsg(
          'Your running Pui Desktop instance is from an earlier build. Quit (⌘Q) and reopen Pui Desktop to enable live provider rate limits.',
        );
      } else {
        setErrorMsg(msg);
      }
    } finally {
      setLoading(false);
    }
  }, [transport, onRefreshSession, fetchUsage]);

  useEffect(() => {
    if (!visible || !transport) return;
    let cancelled = false;
    (async () => {
      setLoading(true);
      const data = await fetchUsage();
      if (cancelled) return;
      if (data && data.providers.length === 0 && !data.data) {
        // Auto-trigger a live host fetch if Desktop hasn't populated usage yet.
        await triggerRefreshAndPoll();
      } else {
        setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [visible, transport, fetchUsage, triggerRefreshAndPoll]);

  const handleRefresh = () => {
    void triggerRefreshAndPoll();
  };

  const tokens = detail?.context_tokens ?? 0;
  const window = detail?.context_window ?? 0;
  const pct = window > 0 ? Math.min(100, Math.round((tokens / window) * 100)) : 0;
  const providers = usage?.providers ?? [];

  return (
    <Modal visible={visible} transparent animationType="slide" onRequestClose={onClose}>
      <Pressable style={styles.backdrop} onPress={onClose}>
        <Pressable
          onPress={(e) => e.stopPropagation()}
          style={[styles.sheet, { backgroundColor: colors.bg, borderColor: colors.border }]}
        >
          <View style={[styles.handle, { backgroundColor: colors.border }]} />
          <View style={styles.header}>
            <Pressable onPress={() => void handleRefresh()} style={styles.iconBtn}>
              {loading ? (
                <ActivityIndicator size="small" color={colors.accent} />
              ) : (
                <Ionicons name="refresh" size={19} color={colors.muted} />
              )}
            </Pressable>
            <Text style={[styles.title, { color: colors.text }]}>Usage &amp; Rate Limits</Text>
            <Pressable onPress={onClose} style={styles.iconBtn}>
              <Ionicons name="close" size={21} color={colors.muted} />
            </Pressable>
          </View>

          <View style={styles.scrollWrap}>
            <ScrollView {...bindings} contentContainerStyle={styles.body}>
              {/* Context window (when opened from a chat) */}
              {detail ? (
                <>
                  <Text style={[styles.section, { color: colors.dim }]}>CONTEXT</Text>
                  <View
                    style={[
                      styles.card,
                      { backgroundColor: colors.surface, borderColor: colors.border },
                    ]}
                  >
                    <View style={styles.rowBetween}>
                      <Text numberOfLines={1} style={[styles.cardTitle, { color: colors.text }]}>
                        Context window
                      </Text>
                      <Text numberOfLines={1} style={[styles.pctText, { color: colors.muted }]}>
                        {pct}% used
                      </Text>
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
                </>
              ) : null}

              {/* Provider Subscription Rate Limits */}
              <Text style={[styles.section, { color: colors.dim }]}>PROVIDER RATE LIMITS</Text>
              {errorMsg ? (
                <View
                  style={[
                    styles.card,
                    { backgroundColor: colors.surface, borderColor: colors.red },
                  ]}
                >
                  <Text style={{ color: colors.red, fontSize: 12.5, lineHeight: 18 }}>
                    {errorMsg}
                  </Text>
                </View>
              ) : providers.length === 0 ? (
                <View
                  style={[
                    styles.card,
                    { backgroundColor: colors.surface, borderColor: colors.border },
                  ]}
                >
                  <View style={{ flexDirection: 'row', alignItems: 'center', gap: 10 }}>
                    {loading ? <ActivityIndicator size="small" color={colors.accent} /> : null}
                    <Text style={{ color: colors.muted, fontSize: 13, flex: 1 }}>
                      {loading
                        ? 'Loading provider rate limits from Pui Desktop…'
                        : 'No provider subscription limits available. Tap ↻ above to retry.'}
                    </Text>
                  </View>
                </View>
              ) : (
                providers.map((prov) => (
                  <ProviderLimitCard
                    key={prov.id}
                    provider={prov}
                    colors={colors}
                    modelsExpanded={Boolean(expandedModels[prov.id])}
                    onToggleModels={() =>
                      setExpandedModels((prev) => ({ ...prev, [prov.id]: !prev[prov.id] }))
                    }
                  />
                ))
              )}

              {/* LiteLLM Budget (if configured) */}
              {usage?.data ? (
                <>
                  <Text style={[styles.section, { color: colors.dim }]}>GATEWAY BUDGET</Text>
                  <View
                    style={[
                      styles.card,
                      { backgroundColor: colors.surface, borderColor: colors.border },
                    ]}
                  >
                    <View style={styles.rowBetween}>
                      <Text style={[styles.cardTitle, { color: colors.text }]}>
                        {usage.data.provider || 'LiteLLM'}
                      </Text>
                      <Text style={[styles.pctText, { color: colors.muted }]}>
                        ${usage.data.spend.toFixed(2)}
                        {usage.data.max ? ` / $${usage.data.max.toFixed(2)}` : ''}
                      </Text>
                    </View>
                    {usage.data.max && usage.data.max > 0 ? (
                      <View style={[styles.track, { backgroundColor: colors.surfaceElevated }]}>
                        <View
                          style={[
                            styles.fill,
                            {
                              width: `${Math.min(100, Math.round((usage.data.spend / usage.data.max) * 100))}%`,
                              backgroundColor: colors.accent,
                            },
                          ]}
                        />
                      </View>
                    ) : null}
                    {usage.data.reset ? (
                      <Text style={[styles.subText, { color: colors.muted }]}>
                        Resets {usage.data.reset}
                      </Text>
                    ) : null}
                  </View>
                </>
              ) : null}

              {/* Session info (when opened from a chat) */}
              {detail ? (
                <>
                  <Text style={[styles.section, { color: colors.dim }]}>SESSION</Text>
                  <View
                    style={[
                      styles.card,
                      { backgroundColor: colors.surface, borderColor: colors.border },
                    ]}
                  >
                    <View style={styles.metaRow}>
                      <Text style={{ color: colors.muted, fontSize: 13 }}>Origin</Text>
                      <OriginBadge origin={detail.origin} />
                    </View>
                    <View style={[styles.divider, { backgroundColor: colors.hairline }]} />
                    <View style={styles.metaRow}>
                      <Text style={{ color: colors.muted, fontSize: 13 }}>Model</Text>
                      <Text
                        numberOfLines={1}
                        style={{ color: colors.text, fontSize: 13, fontWeight: '600', maxWidth: 210 }}
                      >
                        {detail.model_name || detail.model || '—'}
                      </Text>
                    </View>
                    <View style={[styles.divider, { backgroundColor: colors.hairline }]} />
                    <View style={styles.metaRow}>
                      <Text style={{ color: colors.muted, fontSize: 13 }}>Session cost</Text>
                      <Text style={{ color: colors.text, fontSize: 13, fontWeight: '600' }}>
                        ${detail.cost.toFixed(4)}
                      </Text>
                    </View>
                  </View>
                </>
              ) : null}
            </ScrollView>
            <CustomScrollbarOverlay {...metrics} />
          </View>
        </Pressable>
      </Pressable>
    </Modal>
  );
}

function ProviderLimitCard({
  provider,
  colors,
  modelsExpanded,
  onToggleModels,
}: {
  provider: ProviderUsage;
  colors: Palette;
  modelsExpanded: boolean;
  onToggleModels: () => void;
}) {
  return (
    <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
      <View style={styles.rowBetween}>
        <Text numberOfLines={1} style={[styles.cardTitle, { color: colors.text, flex: 1 }]}>
          {provider.name}
        </Text>
        {provider.plan ? (
          <View style={[styles.planBadge, { backgroundColor: colors.surfaceElevated }]}>
            <Text style={{ color: colors.muted, fontSize: 10.5, fontWeight: '600' }}>
              {provider.plan}
            </Text>
          </View>
        ) : null}
      </View>

      {provider.error ? (
        <Text style={{ color: colors.red, fontSize: 12 }}>{provider.error}</Text>
      ) : null}

      {provider.windows.map((win, idx) => (
        <WindowBar key={`${win.label}-${idx}`} window={win} colors={colors} />
      ))}

      {provider.models.length > 0 ? (
        <>
          <Pressable onPress={onToggleModels} style={styles.modelsToggle}>
            <Ionicons
              name={modelsExpanded ? 'chevron-down' : 'chevron-forward'}
              size={13}
              color={colors.muted}
            />
            <Text style={{ color: colors.muted, fontSize: 12, fontWeight: '600' }}>
              Per-model limits ({provider.models.length})
            </Text>
          </Pressable>
          {modelsExpanded ? (
            <View style={{ gap: 10, paddingTop: 4 }}>
              {provider.models.map((win, idx) => (
                <WindowBar key={`m-${win.label}-${idx}`} window={win} colors={colors} />
              ))}
            </View>
          ) : null}
        </>
      ) : null}

      {provider.notes.length > 0 ? (
        <Text style={{ color: colors.dim, fontSize: 11.5, lineHeight: 16 }}>
          {provider.notes.join(' · ')}
        </Text>
      ) : null}
    </View>
  );
}

function WindowBar({ window, colors }: { window: UsageWindow; colors: Palette }) {
  const pct = Math.max(0, Math.min(100, Math.round(window.used)));
  const reset = formatResetTime(window.resets);
  const barColor = pct >= 90 ? colors.red : pct >= 70 ? colors.amber : colors.accent;

  return (
    <View style={{ gap: 5 }}>
      <View style={styles.rowBetween}>
        <Text numberOfLines={1} style={{ color: colors.text, fontSize: 13, fontWeight: '500', flex: 1 }}>
          {window.label}
        </Text>
        <Text numberOfLines={1} style={{ color: colors.muted, fontSize: 12, fontWeight: '600' }}>
          {pct}% used
        </Text>
      </View>
      <View style={[styles.track, { backgroundColor: colors.surfaceElevated }]}>
        <View
          style={[
            styles.fill,
            { width: `${Math.max(pct, pct > 0 ? 3 : 0)}%`, backgroundColor: barColor },
          ]}
        />
      </View>
      {reset || window.detail ? (
        <Text style={{ color: colors.dim, fontSize: 11.5 }}>
          {[reset, window.detail].filter(Boolean).join(' · ')}
        </Text>
      ) : null}
    </View>
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
    paddingBottom: 20,
    maxHeight: '85%',
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
    paddingVertical: 6,
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
  scrollWrap: {
    maxHeight: 560,
    position: 'relative',
  },
  body: {
    paddingHorizontal: 16,
    paddingTop: 6,
    paddingBottom: 22,
    gap: 10,
  },
  section: {
    fontSize: 11,
    fontWeight: '700',
    letterSpacing: 0.7,
    marginTop: 8,
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
    gap: 8,
  },
  cardTitle: {
    fontSize: 14.5,
    fontWeight: '600',
  },
  pctText: {
    fontSize: 12.5,
    fontWeight: '600',
  },
  planBadge: {
    paddingHorizontal: 8,
    paddingVertical: 2.5,
    borderRadius: 999,
  },
  track: {
    height: 7,
    borderRadius: 4,
    overflow: 'hidden',
  },
  fill: {
    height: 7,
    borderRadius: 4,
  },
  subText: {
    fontSize: 12,
  },
  modelsToggle: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 5,
    paddingTop: 2,
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
