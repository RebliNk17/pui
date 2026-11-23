import React, { useMemo, useState } from 'react';
import {
  Modal,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import type { ModelItem } from '../protocol/types';
import { usePreferences } from '../store/preferencesStore';
import { usePalette } from '../theme/colors';
import { CustomScrollbarOverlay, useCustomScrollbar } from './CustomScrollbar';

interface Props {
  visible: boolean;
  models: ModelItem[];
  currentModelId: string;
  currentThinking: string;
  thinkingLevels: string[];
  onClose: () => void;
  onSelectModel: (provider: string, modelId: string) => Promise<void>;
  onSelectThinking: (level: string) => Promise<void>;
}

const DEFAULT_LEVELS = ['off', 'low', 'medium', 'high', 'xhigh'];

export function formatLevelLabel(level: string): string {
  switch (level.toLowerCase()) {
    case 'off':
      return 'Off';
    case 'minimal':
      return 'Minimal';
    case 'low':
      return 'Low';
    case 'medium':
      return 'Medium';
    case 'high':
      return 'High';
    case 'xhigh':
      return 'Max';
    default:
      return level.charAt(0).toUpperCase() + level.slice(1);
  }
}

export function modelKey(model: Pick<ModelItem, 'provider' | 'id'>): string {
  return `${model.provider}/${model.id}`;
}

export function ModelEffortSheet({
  visible,
  models,
  currentModelId,
  currentThinking,
  thinkingLevels,
  onClose,
  onSelectModel,
  onSelectThinking,
}: Props) {
  const colors = usePalette();
  const { isFavoriteModel, toggleFavoriteModel, updatePrefs } = usePreferences();
  const [view, setView] = useState<'models' | 'effort'>('models');
  const [query, setQuery] = useState('');
  const [selectedProvider, setSelectedProvider] = useState<string>('All');
  const { bindings, metrics } = useCustomScrollbar();

  const levels = thinkingLevels.length > 0 ? thinkingLevels : DEFAULT_LEVELS;

  const providers = useMemo(() => {
    const set = new Set<string>();
    for (const m of models) {
      if (m.provider) set.add(m.provider);
    }
    return ['All', ...Array.from(set).sort((a, b) => a.localeCompare(b))];
  }, [models]);

  const { favorites, others } = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = models.filter((m) => {
      if (selectedProvider !== 'All' && m.provider !== selectedProvider) return false;
      if (!q) return true;
      return (
        m.id.toLowerCase().includes(q) ||
        m.name.toLowerCase().includes(q) ||
        m.provider.toLowerCase().includes(q)
      );
    });
    const favs: ModelItem[] = [];
    const rest: ModelItem[] = [];
    for (const m of filtered) {
      if (isFavoriteModel(modelKey(m))) favs.push(m);
      else rest.push(m);
    }
    return { favorites: favs, others: rest };
  }, [models, query, selectedProvider, isFavoriteModel]);

  const closeAll = () => {
    setView('models');
    setQuery('');
    onClose();
  };

  const renderModelRow = (item: ModelItem, idx: number, total: number) => {
    const key = modelKey(item);
    const selected = item.id === currentModelId;
    const fav = isFavoriteModel(key);
    return (
      <Pressable
        key={key}
        onPress={() => {
          void onSelectModel(item.provider, item.id);
          closeAll();
        }}
        style={[
          styles.modelRow,
          {
            backgroundColor: selected ? colors.accentBg : colors.surface,
            borderColor: colors.border,
            borderTopLeftRadius: idx === 0 ? 14 : 0,
            borderTopRightRadius: idx === 0 ? 14 : 0,
            borderBottomLeftRadius: idx === total - 1 ? 14 : 0,
            borderBottomRightRadius: idx === total - 1 ? 14 : 0,
            borderTopWidth: idx === 0 ? 1 : StyleSheet.hairlineWidth,
          },
        ]}
      >
        <View style={{ flex: 1 }}>
          <Text numberOfLines={1} style={[styles.rowTitle, { color: colors.text }]}>
            {item.name || item.id}
          </Text>
          <Text numberOfLines={1} style={{ color: colors.muted, fontSize: 11.5, marginTop: 2 }}>
            {item.provider}
            {item.context_window > 0 ? ` · ${Math.round(item.context_window / 1000)}k ctx` : ''}
          </Text>
        </View>
        <View style={styles.rowRight}>
          {selected ? <Ionicons name="checkmark" size={18} color={colors.accent} /> : null}
          <Pressable
            onPress={() => toggleFavoriteModel(key)}
            hitSlop={8}
            style={styles.starBtn}
          >
            <Ionicons
              name={fav ? 'star' : 'star-outline'}
              size={18}
              color={fav ? colors.amber : colors.dim}
            />
          </Pressable>
        </View>
      </Pressable>
    );
  };

  return (
    <Modal visible={visible} transparent animationType="slide" onRequestClose={closeAll}>
      <View style={styles.backdrop}>
        <Pressable style={StyleSheet.absoluteFill} onPress={closeAll} />
        <View
          style={[styles.sheet, { backgroundColor: colors.bg, borderColor: colors.border }]}
        >
          <View style={[styles.handle, { backgroundColor: colors.border }]} />
          <View style={styles.header}>
            {view === 'effort' ? (
              <Pressable onPress={() => setView('models')} style={styles.iconBtn}>
                <Ionicons name="arrow-back" size={20} color={colors.text} />
              </Pressable>
            ) : (
              <View style={styles.iconBtn} />
            )}
            <Text style={[styles.title, { color: colors.text }]}>
              {view === 'effort' ? 'Effort' : 'Select model'}
            </Text>
            <Pressable onPress={closeAll} style={styles.iconBtn}>
              <Ionicons name="close" size={21} color={colors.muted} />
            </Pressable>
          </View>

          {view === 'effort' ? (
            <View style={styles.effortBody}>
              <View
                style={[
                  styles.groupCard,
                  { backgroundColor: colors.surface, borderColor: colors.border },
                ]}
              >
                {levels.map((lvl, idx) => {
                  const selected =
                    lvl.toLowerCase() === (currentThinking || 'medium').toLowerCase();
                  const isRecommended = lvl.toLowerCase() === 'medium';
                  return (
                    <Pressable
                      key={lvl}
                      onPress={() => {
                        updatePrefs({ defaultEffort: lvl });
                        void onSelectThinking(lvl);
                        setView('models');
                      }}
                      style={[
                        styles.effortRow,
                        idx > 0 && {
                          borderTopWidth: StyleSheet.hairlineWidth,
                          borderTopColor: colors.hairline,
                        },
                      ]}
                    >
                      <View style={styles.effortLeft}>
                        <Text style={[styles.rowTitle, { color: colors.text }]}>
                          {formatLevelLabel(lvl)}
                        </Text>
                        {isRecommended ? (
                          <View
                            style={[styles.recBadge, { backgroundColor: colors.surfaceElevated }]}
                          >
                            <Text
                              style={{ color: colors.muted, fontSize: 10.5, fontWeight: '600' }}
                            >
                              Recommended
                            </Text>
                          </View>
                        ) : null}
                      </View>
                      {selected ? (
                        <Ionicons name="checkmark" size={18} color={colors.accent} />
                      ) : null}
                    </Pressable>
                  );
                })}
              </View>
              <Text style={[styles.hint, { color: colors.muted }]}>
                Higher effort means deeper reasoning, but takes longer and uses more tokens.
              </Text>
            </View>
          ) : (
            <View style={{ maxHeight: 540 }}>
              <View style={styles.searchWrap}>
                <View
                  style={[
                    styles.searchBox,
                    { backgroundColor: colors.surface, borderColor: colors.border },
                  ]}
                >
                  <Ionicons name="search" size={15} color={colors.dim} />
                  <TextInput
                    value={query}
                    onChangeText={setQuery}
                    placeholder="Search all models or providers…"
                    placeholderTextColor={colors.dim}
                    style={[styles.searchInput, { color: colors.text }]}
                  />
                  {query ? (
                    <Pressable onPress={() => setQuery('')}>
                      <Ionicons name="close-circle" size={16} color={colors.dim} />
                    </Pressable>
                  ) : null}
                </View>

                {providers.length > 2 ? (
                  <ScrollView
                    horizontal
                    showsHorizontalScrollIndicator={false}
                    contentContainerStyle={styles.providerRow}
                  >
                    {providers.map((prov) => {
                      const active = prov === selectedProvider;
                      return (
                        <Pressable
                          key={prov}
                          onPress={() => setSelectedProvider(prov)}
                          style={[
                            styles.providerChip,
                            {
                              backgroundColor: active ? colors.accentBg : colors.surface,
                              borderColor: active ? colors.accent : colors.border,
                            },
                          ]}
                        >
                          <Text
                            style={{
                              color: active ? colors.accent : colors.muted,
                              fontSize: 12,
                              fontWeight: '600',
                            }}
                          >
                            {prov}
                          </Text>
                        </Pressable>
                      );
                    })}
                  </ScrollView>
                ) : null}
              </View>

              <View style={{ maxHeight: 410, position: 'relative' }}>
                <ScrollView
                  {...bindings}
                  contentContainerStyle={{ paddingHorizontal: 14, paddingBottom: 20 }}
                >
                  <Pressable
                    onPress={() => setView('effort')}
                    style={[
                      styles.effortTrigger,
                      { backgroundColor: colors.surface, borderColor: colors.border },
                    ]}
                  >
                    <Text
                      numberOfLines={1}
                      style={[styles.rowTitle, { color: colors.text, flexShrink: 1 }]}
                    >
                      Thinking effort
                    </Text>
                    <View style={styles.effortTriggerRight}>
                      <Text
                        numberOfLines={1}
                        style={{ color: colors.muted, fontSize: 13.5 }}
                      >
                        {formatLevelLabel(currentThinking || 'medium')}
                      </Text>
                      <Ionicons name="chevron-forward" size={16} color={colors.dim} />
                    </View>
                  </Pressable>

                  {favorites.length > 0 ? (
                    <>
                      <Text style={[styles.section, { color: colors.amber }]}>
                        ★ Favorites / Pinned ({favorites.length})
                      </Text>
                      {favorites.map((m, idx) => renderModelRow(m, idx, favorites.length))}
                    </>
                  ) : null}

                  <Text style={[styles.section, { color: colors.muted }]}>
                    {selectedProvider === 'All'
                      ? `All models (${others.length})`
                      : `${selectedProvider} (${others.length})`}
                  </Text>
                  {others.length > 0 ? (
                    others.map((m, idx) => renderModelRow(m, idx, others.length))
                  ) : (
                    <Text style={{ color: colors.dim, padding: 14, textAlign: 'center' }}>
                      {favorites.length > 0
                        ? 'All matching models are pinned above.'
                        : 'No matching models.'}
                    </Text>
                  )}
                </ScrollView>
                <CustomScrollbarOverlay {...metrics} />
              </View>
            </View>
          )}
        </View>
      </View>
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
    paddingBottom: 22,
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
  searchWrap: {
    paddingHorizontal: 14,
    gap: 8,
    marginBottom: 8,
  },
  searchBox: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    borderWidth: 1,
    borderRadius: 11,
    paddingHorizontal: 10,
    height: 40,
  },
  searchInput: {
    flex: 1,
    fontSize: 13.5,
    textAlign: 'left',
  },
  providerRow: {
    gap: 6,
    paddingVertical: 2,
  },
  providerChip: {
    paddingHorizontal: 11,
    paddingVertical: 5,
    borderRadius: 999,
    borderWidth: 1,
  },
  effortTrigger: {
    borderRadius: 14,
    borderWidth: 1,
    paddingHorizontal: 15,
    paddingVertical: 13,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
  },
  effortTriggerRight: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  section: {
    fontSize: 11.5,
    fontWeight: '700',
    marginTop: 14,
    marginBottom: 6,
    marginLeft: 2,
  },
  modelRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 15,
    paddingVertical: 12,
    borderLeftWidth: 1,
    borderRightWidth: 1,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  rowRight: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  starBtn: {
    padding: 4,
  },
  effortBody: {
    paddingHorizontal: 14,
    paddingTop: 8,
    gap: 12,
  },
  groupCard: {
    borderRadius: 14,
    borderWidth: 1,
    overflow: 'hidden',
  },
  effortRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 16,
    paddingVertical: 15,
  },
  effortLeft: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  rowTitle: {
    fontSize: 14.5,
    fontWeight: '500',
    textAlign: 'left',
  },
  recBadge: {
    paddingHorizontal: 8,
    paddingVertical: 3,
    borderRadius: 999,
  },
  hint: {
    fontSize: 12.5,
    lineHeight: 18,
    paddingHorizontal: 4,
    textAlign: 'left',
  },
});
