import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ActivityIndicator,
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
  type GalleryImage,
  MAX_ZOOM_SCALE,
  MIN_ZOOM_SCALE,
  nextZoomStep,
  resolveSwipeAction,
  touchDistance,
  touchMidpoint,
  zoomAroundFocalPoint,
  type ZoomState,
} from '../utils/imageZoom';

interface Props {
  uri?: string | null;
  title?: string;
  images?: GalleryImage[];
  initialIndex?: number;
  onClose: () => void;
}

const DOUBLE_TAP_WINDOW_MS = 280;
const DOUBLE_TAP_MAX_DIST = 36;

export function ImageModal({ uri, title, images, initialIndex = 0, onClose }: Props) {
  const gallery = useMemo<GalleryImage[]>(() => {
    if (images && images.length > 0) return images;
    if (uri) return [{ uri, title }];
    return [];
  }, [images, uri, title]);

  const visible = gallery.length > 0;
  const clampedInitial = Math.max(0, Math.min(initialIndex, Math.max(0, gallery.length - 1)));
  const [currentIndex, setCurrentIndex] = useState(clampedInitial);

  const galleryRef = useRef(gallery);
  galleryRef.current = gallery;
  const indexRef = useRef(currentIndex);
  indexRef.current = currentIndex;

  const currentItem = gallery[currentIndex] ?? gallery[0] ?? null;
  const activeUri = currentItem?.uri ?? null;
  const activeTitle = currentItem?.title ?? title ?? 'Image preview';

  const scaleAnim = useRef(new Animated.Value(MIN_ZOOM_SCALE)).current;
  const txAnim = useRef(new Animated.Value(0)).current;
  const tyAnim = useRef(new Animated.Value(0)).current;

  const stateRef = useRef<ZoomState>({ scale: MIN_ZOOM_SCALE, tx: 0, ty: 0 });
  const stageRef = useRef<React.ComponentRef<typeof View> | null>(null);
  const viewportRef = useRef({ x: 0, y: 0, width: 0, height: 0 });
  const [zoomPercent, setZoomPercent] = useState(100);
  const shownPercentRef = useRef(100);
  const [imgLoading, setImgLoading] = useState(false);
  const [imgError, setImgError] = useState<string | null>(null);
  const [retryKey, setRetryKey] = useState(0);

  // Gesture bookkeeping refs (never trigger React re-renders per frame).
  const modeRef = useRef<'none' | 'pan' | 'pinch' | 'swipe'>('none');
  const panStartRef = useRef({ x: 0, y: 0, tx: 0, ty: 0 });
  const swipeDxRef = useRef(0);
  const pinchStartRef = useRef({
    dist: 1,
    midX: 0,
    midY: 0,
    state: { scale: 1, tx: 0, ty: 0 } as ZoomState,
  });
  const movedRef = useRef(false);
  const lastTapRef = useRef<{ time: number; x: number; y: number } | null>(null);

  const syncPercent = useCallback((scale: number, force = false) => {
    const pct = Math.round(scale * 100);
    if (force || Math.abs(pct - shownPercentRef.current) >= 5) {
      shownPercentRef.current = pct;
      setZoomPercent(pct);
    }
  }, []);

  const applyImmediate = useCallback(
    (next: ZoomState, forcePercent = false) => {
      stateRef.current = next;
      scaleAnim.setValue(next.scale);
      txAnim.setValue(next.tx);
      tyAnim.setValue(next.ty);
      syncPercent(next.scale, forcePercent);
    },
    [scaleAnim, txAnim, tyAnim, syncPercent],
  );

  const animateTo = useCallback(
    (next: ZoomState, duration = 180) => {
      stateRef.current = next;
      syncPercent(next.scale, true);
      Animated.parallel([
        Animated.timing(scaleAnim, {
          toValue: next.scale,
          duration,
          useNativeDriver: true,
        }),
        Animated.timing(txAnim, {
          toValue: next.tx,
          duration,
          useNativeDriver: true,
        }),
        Animated.timing(tyAnim, {
          toValue: next.ty,
          duration,
          useNativeDriver: true,
        }),
      ]).start();
    },
    [scaleAnim, txAnim, tyAnim, syncPercent],
  );

  // Sync current index when modal opens or caller changes initialIndex/images.
  useEffect(() => {
    setCurrentIndex(clampedInitial);
    indexRef.current = clampedInitial;
  }, [clampedInitial, visible]);

  // Reset zoom and loading state whenever the active image changes.
  useEffect(() => {
    applyImmediate({ scale: MIN_ZOOM_SCALE, tx: 0, ty: 0 }, true);
    lastTapRef.current = null;
    modeRef.current = 'none';
    swipeDxRef.current = 0;
    setImgError(null);
    setImgLoading(Boolean(activeUri) && !activeUri?.startsWith('data:'));
    setRetryKey(0);
  }, [activeUri, applyImmediate]);

  const navigateToIndex = useCallback(
    (nextIdx: number, direction: 'next' | 'prev') => {
      const total = galleryRef.current.length;
      if (nextIdx < 0 || nextIdx >= total || nextIdx === indexRef.current) return;
      const w = viewportRef.current.width || 360;
      const exitX = direction === 'next' ? -w : w;
      const enterX = direction === 'next' ? w * 0.35 : -w * 0.35;

      Animated.timing(txAnim, {
        toValue: exitX,
        duration: 110,
        useNativeDriver: true,
      }).start(() => {
        indexRef.current = nextIdx;
        setCurrentIndex(nextIdx);
        stateRef.current = { scale: MIN_ZOOM_SCALE, tx: enterX, ty: 0 };
        scaleAnim.setValue(MIN_ZOOM_SCALE);
        tyAnim.setValue(0);
        txAnim.setValue(enterX);
        syncPercent(MIN_ZOOM_SCALE, true);
        Animated.timing(txAnim, {
          toValue: 0,
          duration: 150,
          useNativeDriver: true,
        }).start(() => {
          stateRef.current = { scale: MIN_ZOOM_SCALE, tx: 0, ty: 0 };
        });
      });
    },
    [scaleAnim, txAnim, tyAnim, syncPercent],
  );

  const measureStageOrigin = useCallback(() => {
    stageRef.current?.measureInWindow((x, y, width, height) => {
      if (width > 0 && height > 0) {
        viewportRef.current = { x, y, width, height };
      }
    });
  }, []);

  const handleStageLayout = useCallback(
    (e: LayoutChangeEvent) => {
      const { width, height } = e.nativeEvent.layout;
      viewportRef.current = { ...viewportRef.current, width, height };
      measureStageOrigin();
      const cur = stateRef.current;
      const clamped = clampTranslation(cur.tx, cur.ty, cur.scale, width, height);
      if (clamped.tx !== cur.tx || clamped.ty !== cur.ty) {
        applyImmediate({ scale: cur.scale, tx: clamped.tx, ty: clamped.ty }, true);
      }
    },
    [applyImmediate, measureStageOrigin],
  );

  const touchToStagePoint = useCallback((touch: { pageX: number; pageY: number }) => {
    const vp = viewportRef.current;
    return { x: touch.pageX - vp.x, y: touch.pageY - vp.y };
  }, []);

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
        onStartShouldSetPanResponderCapture: () => true,
        onMoveShouldSetPanResponder: (_, gs) =>
          gs.numberActiveTouches >= 2 ||
          Math.abs(gs.dx) > 4 ||
          Math.abs(gs.dy) > 4,
        onMoveShouldSetPanResponderCapture: (_, gs) => gs.numberActiveTouches >= 2,
        onPanResponderTerminationRequest: () => false,
        onPanResponderGrant: (evt) => {
          movedRef.current = false;
          swipeDxRef.current = 0;
          const touches = evt.nativeEvent.touches;
          if (touches.length >= 2) {
            const p0 = touchToStagePoint(touches[0]);
            const p1 = touchToStagePoint(touches[1]);
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
            const p0 = touchToStagePoint(touches[0]);
            const p1 = touchToStagePoint(touches[1]);
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
            if (modeRef.current !== 'pan' && modeRef.current !== 'swipe') {
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
            if (cur.scale > MIN_ZOOM_SCALE + 0.03) {
              // Pan zoomed image within bounds.
              const clamped = clampTranslation(
                panStartRef.current.tx + dx,
                panStartRef.current.ty + dy,
                cur.scale,
                width,
                height,
              );
              applyImmediate({ scale: cur.scale, tx: clamped.tx, ty: clamped.ty });
            } else if (galleryRef.current.length > 1) {
              // At 1x zoom with multiple images: horizontal swipe switches gallery images.
              if (modeRef.current !== 'swipe' && Math.abs(dx) > 8 && Math.abs(dx) > Math.abs(dy)) {
                modeRef.current = 'swipe';
              }
              if (modeRef.current === 'swipe') {
                const idx = indexRef.current;
                const total = galleryRef.current.length;
                const atEdge = (idx === 0 && dx > 0) || (idx === total - 1 && dx < 0);
                const visualDx = atEdge ? dx * 0.28 : dx;
                swipeDxRef.current = dx;
                txAnim.setValue(visualDx);
              }
            }
          }
        },
        onPanResponderRelease: (evt, gs) => {
          const prevMode = modeRef.current;
          modeRef.current = 'none';
          const cur = stateRef.current;
          syncPercent(cur.scale, true);

          if (prevMode === 'swipe') {
            const action = resolveSwipeAction(
              swipeDxRef.current,
              gs.vx,
              viewportRef.current.width || 360,
              indexRef.current,
              galleryRef.current.length,
            );
            swipeDxRef.current = 0;
            if (action === 'next') {
              navigateToIndex(indexRef.current + 1, 'next');
              return;
            }
            if (action === 'prev') {
              navigateToIndex(indexRef.current - 1, 'prev');
              return;
            }
            animateTo({ scale: MIN_ZOOM_SCALE, tx: 0, ty: 0 }, 150);
            return;
          }

          if (cur.scale < 1.03 && (cur.scale !== MIN_ZOOM_SCALE || cur.tx !== 0 || cur.ty !== 0)) {
            animateTo({ scale: MIN_ZOOM_SCALE, tx: 0, ty: 0 });
            return;
          }
          if (prevMode !== 'pinch' && !movedRef.current) {
            const now = Date.now();
            const tap = touchToStagePoint(evt.nativeEvent);
            const prev = lastTapRef.current;
            if (
              prev &&
              now - prev.time <= DOUBLE_TAP_WINDOW_MS &&
              Math.hypot(tap.x - prev.x, tap.y - prev.y) <= DOUBLE_TAP_MAX_DIST
            ) {
              lastTapRef.current = null;
              if (cur.scale > MIN_ZOOM_SCALE + 0.05) {
                animateTo({ scale: MIN_ZOOM_SCALE, tx: 0, ty: 0 });
              } else {
                const { width, height } = viewportRef.current;
                const next = zoomAroundFocalPoint(
                  cur,
                  DOUBLE_TAP_ZOOM_SCALE,
                  tap.x,
                  tap.y,
                  width,
                  height,
                );
                animateTo(next);
              }
            } else {
              lastTapRef.current = { time: now, x: tap.x, y: tap.y };
            }
          }
        },
        onPanResponderTerminate: () => {
          const wasSwipe = modeRef.current === 'swipe';
          modeRef.current = 'none';
          swipeDxRef.current = 0;
          syncPercent(stateRef.current.scale, true);
          if (wasSwipe) {
            animateTo({ scale: MIN_ZOOM_SCALE, tx: 0, ty: 0 }, 150);
          }
        },
      }),
    [animateTo, applyImmediate, navigateToIndex, syncPercent, touchToStagePoint, txAnim],
  );

  const canZoomOut = zoomPercent > Math.round(MIN_ZOOM_SCALE * 100);
  const canZoomIn = zoomPercent < Math.round(MAX_ZOOM_SCALE * 100);
  const hasMultiple = gallery.length > 1;
  const canPrev = currentIndex > 0;
  const canNext = currentIndex < gallery.length - 1;

  return (
    <Modal visible={visible} transparent animationType="fade" onRequestClose={onClose}>
      <SafeAreaView style={styles.backdrop}>
        <View style={styles.topBar}>
          <View style={styles.titleBlock}>
            <Text numberOfLines={1} style={styles.title}>
              {activeTitle}
            </Text>
            {hasMultiple ? (
              <Text style={styles.counterText}>
                {currentIndex + 1} / {gallery.length}
              </Text>
            ) : null}
          </View>
          <Pressable onPress={onClose} style={styles.closeBtn} hitSlop={10}>
            <Ionicons name="close" size={24} color="#fff" />
          </Pressable>
        </View>

        <View
          ref={stageRef}
          collapsable={false}
          style={styles.stage}
          onLayout={handleStageLayout}
          {...panResponder.panHandlers}
        >
          {activeUri ? (
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
              <Image
                key={`${activeUri}:${retryKey}`}
                source={{ uri: activeUri }}
                style={styles.image}
                resizeMode="contain"
                onLoadStart={() => {
                  if (!activeUri.startsWith('data:')) {
                    setImgLoading(true);
                  }
                  setImgError(null);
                }}
                onLoadEnd={() => setImgLoading(false)}
                onError={() => {
                  setImgLoading(false);
                  setImgError('Could not load image from Pui host.');
                }}
              />
            </Animated.View>
          ) : null}
          {imgLoading ? (
            <View pointerEvents="none" style={styles.overlayCenter}>
              <ActivityIndicator size="large" color="#fff" />
            </View>
          ) : null}
          {imgError ? (
            <View style={styles.overlayCenter}>
              <Ionicons name="alert-circle-outline" size={32} color="#f87171" />
              <Text style={styles.errorText}>{imgError}</Text>
              <Pressable
                onPress={() => {
                  setImgError(null);
                  setImgLoading(true);
                  setRetryKey((k) => k + 1);
                }}
                style={styles.retryBtn}
              >
                <Text style={styles.retryText}>Retry</Text>
              </Pressable>
            </View>
          ) : null}
        </View>

        <View style={styles.bottomBar}>
          {hasMultiple && gallery.length <= 12 ? (
            <View style={styles.dotsRow}>
              {gallery.map((item, idx) => (
                <Pressable
                  key={`${item.uri}:${idx}`}
                  onPress={() =>
                    idx !== currentIndex &&
                    navigateToIndex(idx, idx > currentIndex ? 'next' : 'prev')
                  }
                  hitSlop={6}
                  style={[
                    styles.dot,
                    idx === currentIndex && styles.dotActive,
                  ]}
                />
              ))}
            </View>
          ) : null}

          <View style={styles.controlsRow}>
            {hasMultiple ? (
              <Pressable
                onPress={() => navigateToIndex(currentIndex - 1, 'prev')}
                disabled={!canPrev}
                hitSlop={8}
                accessibilityLabel="Previous image"
                style={[styles.navBtn, !canPrev && styles.zoomBtnDisabled]}
              >
                <Ionicons name="chevron-back" size={20} color="#fff" />
              </Pressable>
            ) : null}

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

            {hasMultiple ? (
              <Pressable
                onPress={() => navigateToIndex(currentIndex + 1, 'next')}
                disabled={!canNext}
                hitSlop={8}
                accessibilityLabel="Next image"
                style={[styles.navBtn, !canNext && styles.zoomBtnDisabled]}
              >
                <Ionicons name="chevron-forward" size={20} color="#fff" />
              </Pressable>
            ) : null}
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
  titleBlock: {
    flex: 1,
    marginRight: 12,
    gap: 2,
  },
  title: {
    color: '#fff',
    fontSize: 14,
    fontWeight: '600',
  },
  counterText: {
    color: 'rgba(255,255,255,0.65)',
    fontSize: 11.5,
    fontVariant: ['tabular-nums'],
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
    paddingVertical: 10,
    gap: 10,
    zIndex: 2,
  },
  dotsRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 6,
  },
  dot: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: 'rgba(255,255,255,0.32)',
  },
  dotActive: {
    width: 16,
    backgroundColor: '#fff',
  },
  controlsRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
  },
  navBtn: {
    width: 38,
    height: 38,
    borderRadius: 19,
    backgroundColor: 'rgba(255,255,255,0.14)',
    alignItems: 'center',
    justifyContent: 'center',
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
    opacity: 0.3,
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
  overlayCenter: {
    position: 'absolute',
    top: 0,
    right: 0,
    bottom: 0,
    left: 0,
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: 24,
  },
  errorText: {
    color: '#fca5a5',
    fontSize: 13.5,
    textAlign: 'center',
    marginTop: 10,
  },
  retryBtn: {
    marginTop: 14,
    paddingHorizontal: 18,
    paddingVertical: 8,
    borderRadius: 999,
    backgroundColor: 'rgba(255,255,255,0.16)',
  },
  retryText: {
    color: '#fff',
    fontSize: 13,
    fontWeight: '600',
  },
});
