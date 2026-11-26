import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  clampScale,
  clampTranslation,
  MAX_ZOOM_SCALE,
  MIN_ZOOM_SCALE,
  nextZoomStep,
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
});
