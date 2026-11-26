import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Animated,
  Image,
  type LayoutChangeEvent,
  Modal,
  PanResponder,
  Pressable,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { Ionicons } from '@expo/vector-icons';
import {
  clampScale,
  clampTranslation,
  DOUBLE_TAP_ZOOM_SCALE,
  MAX_ZOOM_SCALE,
  MIN_ZOOM_SCALE,
  nextZoomStep,
  touchDistance,
  touchMidpoint,
  zoomAroundFocalPoint,
  type ZoomState,
} from '../utils/imageZoom';

interface Props {
  uri: string | null;
  title?: string;
  onClose: () => void;
}

const DOUBLE_TAP_WINDOW_MS = 280;
const DOUBLE_TAP_MAX_DIST = 36;

export function ImageModal({ uri, title, onClose }: Props) {
  const scaleAnim = useRef(new Animated.Value(MIN_ZOOM_SCALE)).current;
  const txAnim = useRef(new Animated.Value(0)).current;
  const tyAnim = useRef(new Animated.Value(0)).current;

  const stateRef = useRef<ZoomState>({ scale: MIN_ZOOM_SCALE, tx: 0, ty: 0 });
  const viewportRef = useRef({ width: 0, height: 0 });
  const [zoomPercent, setZoomPercent] = useState(100);

  // Gesture bookkeeping refs (never trigger React re-renders per frame).
  const modeRef = useRef<'none' | 'pan' | 'pinch'>('none');
  const panStartRef = useRef({ x: 0, y: 0, tx: 0, ty: 0 });
  const pinchStartRef = useRef({
    dist: 1,
    midX: 0,
    midY: 0,
    state: { scale: 1, tx: 0, ty: 0 } as ZoomState,
  });
  const movedRef = useRef(false);
  const lastTapRef = useRef<{ time: number; x: number; y: number } | null>(null);

  const applyImmediate = useCallback(
    (next: ZoomState) => {
      stateRef.current = next;
      scaleAnim.setValue(next.scale);
      txAnim.setValue(next.tx);
      tyAnim.setValue(next.ty);
      setZoomPercent(Math.round(next.scale * 100));
    },
    [scaleAnim, txAnim, tyAnim],
  );

  const animateTo = useCallback(
    (next: ZoomState) => {
      stateRef.current = next;
      setZoomPercent(Math.round(next.scale * 100));
      Animated.parallel([
        Animated.timing(scaleAnim, {
          toValue: next.scale,
          duration: 180,
          useNativeDriver: true,
        }),
        Animated.timing(txAnim, {
          toValue: next.tx,
          duration: 180,
          useNativeDriver: true,
        }),
        Animated.timing(tyAnim, {
          toValue: next.ty,
          duration: 180,
          useNativeDriver: true,
        }),
      ]).start();
    },
    [scaleAnim, txAnim, tyAnim],
  );

  // Reset zoom cleanly whenever a different image opens or the modal closes.
  useEffect(() => {
    applyImmediate({ scale: MIN_ZOOM_SCALE, tx: 0, ty: 0 });
    lastTapRef.current = null;
    modeRef.current = 'none';
  }, [uri, applyImmediate]);

  const handleStageLayout = useCallback(
    (e: LayoutChangeEvent) => {
      const { width, height } = e.nativeEvent.layout;
      viewportRef.current = { width, height };
      const cur = stateRef.current;
      const clamped = clampTranslation(cur.tx, cur.ty, cur.scale, width, height);
      if (clamped.tx !== cur.tx || clamped.ty !== cur.ty) {
        applyImmediate({ scale: cur.scale, tx: clamped.tx, ty: clamped.ty });
      }
    },
    [applyImmediate],
  );

  const handleZoomButton = useCallback(
    (direction: 'in' | 'out') => {
      const { width, height } = viewportRef.current;
      const cur = stateRef.current;
      const targetScale = nextZoomStep(cur.scale, direction);
      const next = zoomAroundFocalPoint(cur, targetScale, width / 2, height / 2, width, height);
      animateTo(next);
    },
    [animateTo],
  );

  const handleResetZoom = useCallback(() => {
    animateTo({ scale: MIN_ZOOM_SCALE, tx: 0, ty: 0 });
  }, [animateTo]);

  const panResponder = useMemo(
    () =>
      PanResponder.create({
        onStartShouldSetPanResponder: () => true,
        onMoveShouldSetPanResponder: (_, gs) =>
          gs.numberActiveTouches >= 2 ||
          (stateRef.current.scale > MIN_ZOOM_SCALE + 0.01 &&
            (Math.abs(gs.dx) > 2 || Math.abs(gs.dy) > 2)),
        onPanResponderGrant: (evt) => {
          movedRef.current = false;
          const touches = evt.nativeEvent.touches;
          if (touches.length >= 2) {
            const p0 = { x: touches[0].locationX, y: touches[0].locationY };
            const p1 = { x: touches[1].locationX, y: touches[1].locationY };
            const mid = touchMidpoint(p0, p1);
            modeRef.current = 'pinch';
            pinchStartRef.current = {
              dist: Math.max(1, touchDistance(p0, p1)),
              midX: mid.x,
              midY: mid.y,
              state: { ...stateRef.current },
            };
          } else if (touches.length === 1) {
            modeRef.current = 'pan';
            panStartRef.current = {
              x: touches[0].pageX,
              y: touches[0].pageY,
              tx: stateRef.current.tx,
              ty: stateRef.current.ty,
            };
          }
        },
        onPanResponderMove: (evt) => {
          const touches = evt.nativeEvent.touches;
          const { width, height } = viewportRef.current;
          if (touches.length >= 2) {
            movedRef.current = true;
            const p0 = { x: touches[0].locationX, y: touches[0].locationY };
            const p1 = { x: touches[1].locationX, y: touches[1].locationY };
            const dist = Math.max(1, touchDistance(p0, p1));
            const mid = touchMidpoint(p0, p1);
            if (modeRef.current !== 'pinch') {
              modeRef.current = 'pinch';
              pinchStartRef.current = {
                dist,
                midX: mid.x,
                midY: mid.y,
                state: { ...stateRef.current },
              };
              return;
            }
            const base = pinchStartRef.current;
            const nextScale = clampScale(base.state.scale * (dist / base.dist));
            const zoomed = zoomAroundFocalPoint(
              base.state,
              nextScale,
              base.midX,
              base.midY,
              width,
              height,
            );
            const dx = mid.x - base.midX;
            const dy = mid.y - base.midY;
            const clamped = clampTranslation(
              zoomed.tx + dx,
              zoomed.ty + dy,
              zoomed.scale,
              width,
              height,
            );
            applyImmediate({ scale: zoomed.scale, tx: clamped.tx, ty: clamped.ty });
          } else if (touches.length === 1) {
            const cur = stateRef.current;
            if (modeRef.current !== 'pan') {
              // Seamless transition from 2-finger pinch to 1-finger pan without jumping.
              modeRef.current = 'pan';
              panStartRef.current = {
                x: touches[0].pageX,
                y: touches[0].pageY,
                tx: cur.tx,
                ty: cur.ty,
              };
              return;
            }
            const dx = touches[0].pageX - panStartRef.current.x;
            const dy = touches[0].pageY - panStartRef.current.y;
            if (Math.abs(dx) > 4 || Math.abs(dy) > 4) {
              movedRef.current = true;
            }
            if (cur.scale > MIN_ZOOM_SCALE + 0.01) {
              const clamped = clampTranslation(
                panStartRef.current.tx + dx,
                panStartRef.current.ty + dy,
                cur.scale,
                width,
                height,
              );
              applyImmediate({ scale: cur.scale, tx: clamped.tx, ty: clamped.ty });
            }
          }
        },
        onPanResponderRelease: (evt) => {
          const wasPinch = modeRef.current === 'pinch';
          modeRef.current = 'none';
          const cur = stateRef.current;
          if (cur.scale < 1.03 && (cur.scale !== MIN_ZOOM_SCALE || cur.tx !== 0 || cur.ty !== 0)) {
            animateTo({ scale: MIN_ZOOM_SCALE, tx: 0, ty: 0 });
            return;
          }
          if (!wasPinch && !movedRef.current) {
            const now = Date.now();
            const tapX = evt.nativeEvent.locationX;
            const tapY = evt.nativeEvent.locationY;
            const prev = lastTapRef.current;
            if (
              prev &&
              now - prev.time <= DOUBLE_TAP_WINDOW_MS &&
              Math.hypot(tapX - prev.x, tapY - prev.y) <= DOUBLE_TAP_MAX_DIST
            ) {
              lastTapRef.current = null;
              if (cur.scale > MIN_ZOOM_SCALE + 0.05) {
                animateTo({ scale: MIN_ZOOM_SCALE, tx: 0, ty: 0 });
              } else {
                const { width, height } = viewportRef.current;
                const next = zoomAroundFocalPoint(
                  cur,
                  DOUBLE_TAP_ZOOM_SCALE,
                  tapX,
                  tapY,
                  width,
                  height,
                );
                animateTo(next);
              }
            } else {
              lastTapRef.current = { time: now, x: tapX, y: tapY };
            }
          }
        },
        onPanResponderTerminate: () => {
          modeRef.current = 'none';
        },
      }),
    [animateTo, applyImmediate],
  );

  const canZoomOut = zoomPercent > Math.round(MIN_ZOOM_SCALE * 100);
  const canZoomIn = zoomPercent < Math.round(MAX_ZOOM_SCALE * 100);

  return (
    <Modal visible={Boolean(uri)} transparent animationType="fade" onRequestClose={onClose}>
      <SafeAreaView style={styles.backdrop}>
        <View style={styles.topBar}>
          <Text numberOfLines={1} style={styles.title}>
            {title ?? 'Image preview'}
          </Text>
          <Pressable onPress={onClose} style={styles.closeBtn} hitSlop={10}>
            <Ionicons name="close" size={24} color="#fff" />
          </Pressable>
        </View>

        <View
          style={styles.stage}
          onLayout={handleStageLayout}
          {...panResponder.panHandlers}
        >
          {uri ? (
            <Animated.View
              pointerEvents="none"
              style={[
                styles.imageWrap,
                {
                  transform: [
                    { translateX: txAnim },
                    { translateY: tyAnim },
                    { scale: scaleAnim },
                  ],
                },
              ]}
            >
              <Image source={{ uri }} style={styles.image} resizeMode="contain" />
            </Animated.View>
          ) : null}
        </View>

        <View style={styles.bottomBar}>
          <View style={styles.zoomPill}>
            <Pressable
              onPress={() => handleZoomButton('out')}
              disabled={!canZoomOut}
              hitSlop={8}
              accessibilityLabel="Zoom out"
              style={[styles.zoomBtn, !canZoomOut && styles.zoomBtnDisabled]}
            >
              <Ionicons name="remove" size={18} color="#fff" />
            </Pressable>
            <Pressable
              onPress={handleResetZoom}
              disabled={!canZoomOut}
              hitSlop={6}
              accessibilityLabel="Reset zoom"
              style={styles.zoomReadout}
            >
              <Text style={styles.zoomText}>{zoomPercent}%</Text>
            </Pressable>
            <Pressable
              onPress={() => handleZoomButton('in')}
              disabled={!canZoomIn}
              hitSlop={8}
              accessibilityLabel="Zoom in"
              style={[styles.zoomBtn, !canZoomIn && styles.zoomBtnDisabled]}
            >
              <Ionicons name="add" size={18} color="#fff" />
            </Pressable>
          </View>
        </View>
      </SafeAreaView>
    </Modal>
  );
}

const styles = StyleSheet.create({
  backdrop: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.96)',
    direction: 'ltr',
  },
  topBar: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 16,
    paddingVertical: 10,
    zIndex: 2,
  },
  title: {
    color: '#fff',
    fontSize: 14,
    fontWeight: '600',
    flex: 1,
    marginRight: 12,
  },
  closeBtn: {
    width: 36,
    height: 36,
    borderRadius: 18,
    backgroundColor: 'rgba(255,255,255,0.14)',
    alignItems: 'center',
    justifyContent: 'center',
  },
  stage: {
    flex: 1,
    overflow: 'hidden',
    paddingHorizontal: 8,
  },
  imageWrap: {
    flex: 1,
    width: '100%',
  },
  image: {
    flex: 1,
    width: '100%',
  },
  bottomBar: {
    alignItems: 'center',
    paddingVertical: 12,
    zIndex: 2,
  },
  zoomPill: {
    flexDirection: 'row',
    alignItems: 'center',
    backgroundColor: 'rgba(255,255,255,0.14)',
    borderRadius: 999,
    paddingHorizontal: 6,
    paddingVertical: 4,
    gap: 4,
  },
  zoomBtn: {
    width: 32,
    height: 32,
    borderRadius: 16,
    alignItems: 'center',
    justifyContent: 'center',
  },
  zoomBtnDisabled: {
    opacity: 0.35,
  },
  zoomReadout: {
    minWidth: 52,
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: 6,
  },
  zoomText: {
    color: '#fff',
    fontSize: 12.5,
    fontWeight: '600',
    fontVariant: ['tabular-nums'],
  },
});
