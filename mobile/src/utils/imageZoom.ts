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

export interface GalleryImage {
  uri: string;
  title?: string;
}

export interface GalleryMessageInput {
  text?: string;
  images?: { data: string; mime: string; file?: { name: string } | null }[];
  tools?: {
    name?: string;
    open_target?: string | null;
    images?: { data: string; mime: string; file?: { name: string } | null }[];
  }[];
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

const IMAGE_EXT_RE = /\.(png|jpe?g|gif|webp|bmp)$/i;
const MD_IMAGE_RE = /!\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;

export function isImageFilePath(target: string): boolean {
  return IMAGE_EXT_RE.test((target.split(/[?#]/)[0] ?? '').trim());
}

/**
 * Collect all unique images across a chat transcript in chronological order:
 * user/assistant attached images, inline Markdown images, tool-result images,
 * and `pui_open` image targets.
 */
export function collectChatGalleryImages(
  messages: GalleryMessageInput[],
  resolveLocalUri: (src: string) => string | null,
): GalleryImage[] {
  const out: GalleryImage[] = [];
  const seen = new Set<string>();
  const push = (uri: string | null | undefined, title?: string) => {
    if (!uri || seen.has(uri)) return;
    seen.add(uri);
    out.push({ uri, title });
  };

  for (const msg of messages) {
    if (msg.images) {
      msg.images.forEach((img, idx) => {
        if (img.data) {
          push(
            `data:${img.mime || 'image/png'};base64,${img.data}`,
            img.file?.name || `Image ${idx + 1}`,
          );
        }
      });
    }
    if (msg.text) {
      MD_IMAGE_RE.lastIndex = 0;
      let match: RegExpExecArray | null;
      while ((match = MD_IMAGE_RE.exec(msg.text)) !== null) {
        const alt = (match[1] ?? '').trim();
        const rawSrc = (match[2] ?? '').trim().replace(/^<|>$/g, '');
        if (!rawSrc) continue;
        const uri = resolveLocalUri(rawSrc);
        const fallbackName = rawSrc.split('/').pop() || rawSrc;
        push(uri, alt || fallbackName);
      }
    }
    if (msg.tools) {
      for (const tool of msg.tools) {
        if (tool.images) {
          tool.images.forEach((img) => {
            if (img.data) {
              push(
                `data:${img.mime || 'image/png'};base64,${img.data}`,
                img.file?.name || `${tool.name || 'Tool'} image`,
              );
            }
          });
        }
        if (tool.open_target && isImageFilePath(tool.open_target)) {
          const uri = resolveLocalUri(tool.open_target);
          const label = tool.open_target.split('/').pop() || tool.open_target;
          push(uri, label);
        }
      }
    }
  }
  return out;
}

/** Decide whether a horizontal release at 1x zoom should page next, prev, or snap back. */
export function resolveSwipeAction(
  dx: number,
  vx: number,
  viewportWidth: number,
  currentIndex: number,
  total: number,
): 'next' | 'prev' | 'stay' {
  if (total <= 1) return 'stay';
  const threshold = Math.min(80, Math.max(40, viewportWidth * 0.2));
  if ((dx < -threshold || (dx < -20 && vx < -0.45)) && currentIndex < total - 1) {
    return 'next';
  }
  if ((dx > threshold || (dx > 20 && vx > 0.45)) && currentIndex > 0) {
    return 'prev';
  }
  return 'stay';
}
