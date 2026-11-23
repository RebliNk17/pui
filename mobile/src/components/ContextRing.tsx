import React from 'react';
import { StyleSheet, View } from 'react-native';
import { usePalette } from '../theme/colors';

interface Props {
  /** Fraction between 0 and 1. */
  progress: number;
  size?: number;
}

/**
 * Circular context-window indicator rendered in pure React Native (no SVG dependency).
 */
export function ContextRing({ progress, size = 22 }: Props) {
  const colors = usePalette();
  const clamped = Math.max(0, Math.min(1, Number.isFinite(progress) ? progress : 0));
  const stroke = 2.5;
  const activeColor =
    clamped >= 0.85 ? colors.red : clamped >= 0.65 ? colors.amber : colors.accent;

  const firstHalfDeg = Math.min(clamped, 0.5) * 360;
  const secondHalfDeg = Math.max(0, clamped - 0.5) * 360;

  return (
    <View style={[styles.wrap, { width: size, height: size }]}>
      <View
        style={{
          width: size,
          height: size,
          borderRadius: size / 2,
          borderWidth: stroke,
          borderColor: colors.border,
          position: 'absolute',
        }}
      />
      {/* Right half arc (0% -> 50%) */}
      <View
        style={{
          position: 'absolute',
          width: size / 2,
          height: size,
          right: 0,
          overflow: 'hidden',
        }}
      >
        <View
          style={{
            width: size,
            height: size,
            borderRadius: size / 2,
            borderWidth: stroke,
            borderColor: 'transparent',
            borderTopColor: activeColor,
            borderRightColor: activeColor,
            position: 'absolute',
            right: 0,
            transform: [{ rotate: `${-135 + firstHalfDeg}deg` }],
          }}
        />
      </View>
      {/* Left half arc (50% -> 100%) */}
      {clamped > 0.5 ? (
        <View
          style={{
            position: 'absolute',
            width: size / 2,
            height: size,
            left: 0,
            overflow: 'hidden',
          }}
        >
          <View
            style={{
              width: size,
              height: size,
              borderRadius: size / 2,
              borderWidth: stroke,
              borderColor: 'transparent',
              borderBottomColor: activeColor,
              borderLeftColor: activeColor,
              position: 'absolute',
              left: 0,
              transform: [{ rotate: `${-135 + secondHalfDeg}deg` }],
            }}
          />
        </View>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create({
  wrap: {
    justifyContent: 'center',
    alignItems: 'center',
  },
});
