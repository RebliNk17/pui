export const MIN_ZOOM_SCALE = 1;
export const MAX_ZOOM_SCALE = 6;
export const DOUBLE_TAP_ZOOM_SCALE = 2.5;
export const ZOOM_STEPS = [1, 1.5, 2.5, 4, 6] as const;

export interface ZoomState {
  scale: number;
  tx: number;
  ty: number;
}

export interface Point {
  x: number;
  y: number;
}

export function clampScale(scale: number): number {
  if (!Number.isFinite(scale)) return MIN_ZOOM_SCALE;
  return Math.min(MAX_ZOOM_SCALE, Math.max(MIN_ZOOM_SCALE, scale));
}

export function clampTranslation(
  tx: number,
  ty: number,
  scale: number,
  viewportWidth: number,
  viewportHeight: number,
): { tx: number; ty: number } {
  const s = clampScale(scale);
  if (s <= MIN_ZOOM_SCALE + 0.001 || viewportWidth <= 0 || viewportHeight <= 0) {
    return { tx: 0, ty: 0 };
  }
  const maxTx = (viewportWidth * (s - 1)) / 2;
  const maxTy = (viewportHeight * (s - 1)) / 2;
  return {
    tx: Math.min(maxTx, Math.max(-maxTx, tx)),
    ty: Math.min(maxTy, Math.max(-maxTy, ty)),
  };
}

/**
 * Compute new (scale, tx, ty) when zooming around a screen-space focal point
 * `(focalX, focalY)` inside a viewport of `(viewportWidth, viewportHeight)`,
 * assuming `transform: [{ translateX: tx }, { translateY: ty }, { scale }]`.
 */
export function zoomAroundFocalPoint(
  current: ZoomState,
  targetScale: number,
  focalX: number,
  focalY: number,
  viewportWidth: number,
  viewportHeight: number,
): ZoomState {
  const nextScale = clampScale(targetScale);
  if (nextScale <= MIN_ZOOM_SCALE + 0.001 || viewportWidth <= 0 || viewportHeight <= 0) {
    return { scale: MIN_ZOOM_SCALE, tx: 0, ty: 0 };
  }
  const curScale = Math.max(MIN_ZOOM_SCALE, current.scale);
  const cx = focalX - viewportWidth / 2;
  const cy = focalY - viewportHeight / 2;
  const ratio = nextScale / curScale;
  const rawTx = cx - (cx - current.tx) * ratio;
  const rawTy = cy - (cy - current.ty) * ratio;
  const clamped = clampTranslation(rawTx, rawTy, nextScale, viewportWidth, viewportHeight);
  return { scale: nextScale, tx: clamped.tx, ty: clamped.ty };
}

export function nextZoomStep(currentScale: number, direction: 'in' | 'out'): number {
  const eps = 0.05;
  if (direction === 'in') {
    for (const step of ZOOM_STEPS) {
      if (step > currentScale + eps) return step;
    }
    return MAX_ZOOM_SCALE;
  }
  for (let i = ZOOM_STEPS.length - 1; i >= 0; i -= 1) {
    if (ZOOM_STEPS[i] < currentScale - eps) return ZOOM_STEPS[i];
  }
  return MIN_ZOOM_SCALE;
}

export function touchDistance(a: Point, b: Point): number {
  return Math.hypot(b.x - a.x, b.y - a.y);
}

export function touchMidpoint(a: Point, b: Point): Point {
  return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
}
