import React from 'react';
import { StyleSheet, Text, View } from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import type { SessionOrigin } from '../protocol/types';
import { usePalette } from '../theme/colors';

interface Props {
  origin?: SessionOrigin | null;
  compact?: boolean;
}

export function OriginBadge({ origin, compact = false }: Props) {
  const colors = usePalette();
  const platform = origin?.platform ?? 'desktop';
  const isMobile = platform === 'android' || platform === 'ios';

  const iconName: keyof typeof Ionicons.glyphMap =
    platform === 'android' || platform === 'ios'
      ? 'phone-portrait-outline'
      : platform === 'web'
        ? 'globe-outline'
        : platform === 'cli'
          ? 'terminal-outline'
          : 'laptop-outline';

  const platformLabel =
    platform === 'android'
      ? 'Android'
      : platform === 'ios'
        ? 'iOS'
        : platform === 'web'
          ? 'Web'
          : platform === 'cli'
            ? 'CLI'
            : 'Desktop';

  const text =
    !compact && origin?.device && isMobile
      ? `${platformLabel} · ${origin.device}`
      : platformLabel;

  return (
    <View
      style={[
        styles.badge,
        {
          backgroundColor: isMobile ? colors.accentBg : colors.surfaceElevated,
          borderColor: isMobile ? colors.accent : colors.border,
        },
      ]}
    >
      <Ionicons
        name={iconName}
        size={11}
        color={isMobile ? colors.accent : colors.muted}
        style={styles.icon}
      />
      <Text
        numberOfLines={1}
        style={[styles.label, { color: isMobile ? colors.accent : colors.muted }]}
      >
        {text}
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  badge: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: 6,
    paddingVertical: 2,
    borderRadius: 999,
    borderWidth: StyleSheet.hairlineWidth,
    alignSelf: 'flex-start',
    maxWidth: 180,
  },
  icon: {
    marginRight: 3,
  },
  label: {
    fontSize: 10.5,
    fontWeight: '600',
  },
});
