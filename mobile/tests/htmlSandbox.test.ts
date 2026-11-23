import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { SANDBOX_CSP, wrapSandboxedHtml } from '../src/utils/htmlSandbox.ts';

describe('HTML artifact sandbox wrapper', () => {
  it('wraps HTML fragments with strict CSP and height reporter', () => {
    const doc = wrapSandboxedHtml('<svg><circle r="10"/></svg>', true);
    assert.ok(doc.includes(SANDBOX_CSP));
    assert.ok(doc.includes("connect-src 'none'"));
    assert.ok(doc.includes("frame-src 'none'"));
    assert.ok(doc.includes('<svg><circle r="10"/></svg>'));
    assert.ok(doc.includes('ReactNativeWebView.postMessage'));
  });

  it('injects CSP into full HTML documents without double-wrapping', () => {
    const raw = '<!doctype html><html><head><title>Report</title></head><body><h1>Hi</h1></body></html>';
    const doc = wrapSandboxedHtml(raw, false);
    assert.ok(doc.includes(SANDBOX_CSP));
    assert.equal((doc.match(/<html/gi) ?? []).length, 1);
    assert.ok(doc.includes('<h1>Hi</h1>'));
  });
});
