import React from 'react';
import { Image, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import type { ArtifactItem } from '../protocol/types';
import type { PuiTransport } from '../transport/transport';
import { usePalette } from '../theme/colors';
import { HtmlArtifactCard } from './HtmlArtifactCard';

interface Props {
  cwd: string;
  artifacts: ArtifactItem[];
  transport: PuiTransport | null;
  onOpenImage: (uri: string, title?: string) => void;
  onOpenFile: (path: string) => void;
}

export function ArtifactsView({ cwd, artifacts, transport, onOpenImage, onOpenFile }: Props) {
  const colors = usePalette();

  if (artifacts.length === 0) {
    return (
      <View style={styles.empty}>
        <Ionicons name="layers-outline" size={32} color={colors.dim} />
        <Text style={[styles.emptyTitle, { color: colors.muted }]}>No artifacts yet</Text>
        <Text style={[styles.emptySub, { color: colors.dim }]}>
          HTML visuals, opened reports, and images from this conversation appear here.
        </Text>
      </View>
    );
  }

  return (
    <ScrollView contentContainerStyle={styles.container}>
      {artifacts.map((item) => {
        if (item.kind === 'visual_html' && item.html) {
          return (
            <HtmlArtifactCard
              key={item.id}
              title={item.title}
              html={item.html}
              initialHeight={item.height}
            />
          );
        }
        if (item.kind === 'image') {
          const uri = item.data_base64
            ? `data:${item.mime || 'image/png'};base64,${item.data_base64}`
            : item.path && transport
              ? transport.getRawFileUrl(cwd, item.path)
              : null;
          return (
            <Pressable
              key={item.id}
              onPress={() => uri && onOpenImage(uri, item.title)}
              style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}
            >
              <Text style={[styles.cardTitle, { color: colors.text }]}>{item.title}</Text>
              {uri ? (
                <Image source={{ uri }} style={styles.image} resizeMode="cover" />
              ) : null}
            </Pressable>
          );
        }
        return (
          <Pressable
            key={item.id}
            onPress={() => item.path && onOpenFile(item.path)}
            style={[styles.fileCard, { backgroundColor: colors.surface, borderColor: colors.border }]}
          >
            <Ionicons
              name={item.kind === 'file_html' ? 'code-slash-outline' : 'document-text-outline'}
              size={20}
              color={item.kind === 'file_html' ? colors.amber : colors.accent}
            />
            <View style={{ flex: 1 }}>
              <Text style={[styles.cardTitle, { color: colors.text }]}>{item.title}</Text>
              {item.path ? (
                <Text numberOfLines={1} style={{ color: colors.dim, fontSize: 11.5 }}>
                  {item.path}
                </Text>
              ) : null}
            </View>
            <Ionicons name="chevron-forward" size={16} color={colors.dim} />
          </Pressable>
        );
      })}
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  container: {
    padding: 12,
    gap: 10,
  },
  empty: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
    padding: 28,
    gap: 8,
  },
  emptyTitle: {
    fontSize: 15,
    fontWeight: '600',
  },
  emptySub: {
    fontSize: 12.5,
    textAlign: 'center',
    lineHeight: 18,
  },
  card: {
    borderWidth: 1,
    borderRadius: 10,
    padding: 10,
    gap: 8,
  },
  fileCard: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    borderWidth: 1,
    borderRadius: 10,
    padding: 12,
  },
  cardTitle: {
    fontSize: 13.5,
    fontWeight: '600',
  },
  image: {
    width: '100%',
    height: 190,
    borderRadius: 8,
  },
});
