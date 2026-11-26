import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  clampScale,
  clampTranslation,
  collectChatGalleryImages,
  MAX_ZOOM_SCALE,
  MIN_ZOOM_SCALE,
  nextZoomStep,
  resolveSwipeAction,
  touchDistance,
  touchMidpoint,
  zoomAroundFocalPoint,
} from '../src/utils/imageZoom.ts';

describe('mobile image viewer zoom and pan geometry', () => {
  it('clamps scale and translation within viewport bounds', () => {
    assert.equal(clampScale(0.2), MIN_ZOOM_SCALE);
    assert.equal(clampScale(12), MAX_ZOOM_SCALE);
    assert.equal(clampScale(Number.NaN), MIN_ZOOM_SCALE);

    assert.deepEqual(clampTranslation(50, -40, 1, 400, 800), { tx: 0, ty: 0 });
    // At 3x in 400x800 viewport, max offsets are (400*2)/2=400 and (800*2)/2=800.
    assert.deepEqual(clampTranslation(600, -1000, 3, 400, 800), { tx: 400, ty: -800 });
  });

  it('keeps focal point anchored when zooming in and resets cleanly at 1x', () => {
    const start = { scale: 1, tx: 0, ty: 0 };
    // Tap at (300, 500) in a 400x800 viewport (cx=100, cy=100).
    const zoomed = zoomAroundFocalPoint(start, 2.5, 300, 500, 400, 800);
    assert.equal(zoomed.scale, 2.5);
    assert.equal(zoomed.tx, -150);
    assert.equal(zoomed.ty, -150);

    const reset = zoomAroundFocalPoint(zoomed, 1, 200, 400, 400, 800);
    assert.deepEqual(reset, { scale: 1, tx: 0, ty: 0 });
  });

  it('steps cleanly through zoom presets and measures pinch gestures', () => {
    assert.equal(nextZoomStep(1, 'in'), 1.5);
    assert.equal(nextZoomStep(1.5, 'in'), 2.5);
    assert.equal(nextZoomStep(6, 'in'), 6);
    assert.equal(nextZoomStep(2.5, 'out'), 1.5);
    assert.equal(nextZoomStep(1, 'out'), 1);

    assert.equal(touchDistance({ x: 0, y: 0 }, { x: 3, y: 4 }), 5);
    assert.deepEqual(touchMidpoint({ x: 10, y: 20 }, { x: 30, y: 40 }), { x: 20, y: 30 });
  });

  it('collects deduplicated gallery images across markdown, attachments, and pui_open', () => {
    const gallery = collectChatGalleryImages(
      [
        {
          images: [{ data: 'AAA', mime: 'image/png', file: { name: 'upload.png' } }],
          text: 'Check this',
        },
        {
          text: '1. ![Workflow](/tmp/a.png)\n2. ![Sandbox](/tmp/b.png)',
          tools: [
            { name: 'pui_open', open_target: '/tmp/a.png' }, // duplicate of markdown image
            { name: 'pui_open', open_target: '/tmp/c.png' },
            { name: 'pui_open', open_target: '/tmp/notes.md' }, // non-image ignored
          ],
        },
      ],
      (src) => `http://host/v1/files/raw?path=${src}`,
    );

    assert.deepEqual(gallery, [
      { uri: 'data:image/png;base64,AAA', title: 'upload.png' },
      { uri: 'http://host/v1/files/raw?path=/tmp/a.png', title: 'Workflow' },
      { uri: 'http://host/v1/files/raw?path=/tmp/b.png', title: 'Sandbox' },
      { uri: 'http://host/v1/files/raw?path=/tmp/c.png', title: 'c.png' },
    ]);
  });

  it('resolves horizontal swipe paging at boundaries and velocity thresholds', () => {
    assert.equal(resolveSwipeAction(-100, -0.2, 400, 0, 3), 'next');
    assert.equal(resolveSwipeAction(100, 0.2, 400, 2, 3), 'prev');
    assert.equal(resolveSwipeAction(100, 0.5, 400, 0, 3), 'stay'); // at first image
    assert.equal(resolveSwipeAction(-100, -0.5, 400, 2, 3), 'stay'); // at last image
    assert.equal(resolveSwipeAction(-30, -0.6, 400, 1, 3), 'next'); // quick flick
    assert.equal(resolveSwipeAction(-25, -0.1, 400, 1, 3), 'stay'); // small drift snaps back
  });
});
