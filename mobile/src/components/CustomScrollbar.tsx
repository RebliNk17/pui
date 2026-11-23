import React, { useCallback, useState } from 'react';
import {
  StyleSheet,
  View,
  type LayoutChangeEvent,
  type NativeScrollEvent,
  type NativeSyntheticEvent,
} from 'react-native';
import { usePalette } from '../theme/colors';

export interface ScrollbarBindings {
  showsVerticalScrollIndicator: false;
  scrollEventThrottle: number;
  onLayout: (e: LayoutChangeEvent) => void;
  onContentSizeChange: (_w: number, h: number) => void;
  onScroll: (e: NativeSyntheticEvent<NativeScrollEvent>) => void;
}

export function useCustomScrollbar() {
  const [viewportHeight, setViewportHeight] = useState(0);
  const [contentHeight, setContentHeight] = useState(0);
  const [offsetY, setOffsetY] = useState(0);

  const onLayout = useCallback((e: LayoutChangeEvent) => {
    setViewportHeight(e.nativeEvent.layout.height);
  }, []);

  const onContentSizeChange = useCallback((_w: number, h: number) => {
    setContentHeight(h);
  }, []);

  const onScroll = useCallback((e: NativeSyntheticEvent<NativeScrollEvent>) => {
    setOffsetY(e.nativeEvent.contentOffset.y);
  }, []);

  const bindings: ScrollbarBindings = {
    showsVerticalScrollIndicator: false,
    scrollEventThrottle: 16,
    onLayout,
    onContentSizeChange,
    onScroll,
  };

  return {
    bindings,
    metrics: { viewportHeight, contentHeight, offsetY },
  };
}

export function CustomScrollbarOverlay({
  viewportHeight,
  contentHeight,
  offsetY,
  inverted = false,
}: {
  viewportHeight: number;
  contentHeight: number;
  offsetY: number;
  inverted?: boolean;
}) {
  const colors = usePalette();
  if (viewportHeight <= 0 || contentHeight <= viewportHeight + 8) {
    return null;
  }

  const padding = 8;
  const trackHeight = Math.max(0, viewportHeight - padding * 2);
  const ratio = viewportHeight / contentHeight;
  const thumbHeight = Math.max(32, Math.min(trackHeight * ratio, trackHeight * 0.65));
  const maxScroll = Math.max(1, contentHeight - viewportHeight);
  const clampedOffset = Math.max(0, Math.min(offsetY, maxScroll));
  const maxThumbTop = Math.max(0, trackHeight - thumbHeight);
  const progress = clampedOffset / maxScroll;
  const thumbTop = padding + (inverted ? 1 - progress : progress) * maxThumbTop;

  return (
    <View pointerEvents="none" style={styles.track}>
      <View
        style={[
          styles.thumb,
          {
            height: thumbHeight,
            top: thumbTop,
            backgroundColor: colors.scrollbar,
          },
        ]}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  track: {
    position: 'absolute',
    right: 3,
    top: 0,
    bottom: 0,
    width: 6,
    alignItems: 'center',
  },
  thumb: {
    position: 'absolute',
    width: 3.5,
    borderRadius: 99,
  },
});
