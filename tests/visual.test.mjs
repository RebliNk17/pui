import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';

const raw = await readFile(new URL('../scripts/visual.ts', import.meta.url), 'utf8');
const source = stripTypeScriptTypes(
  raw.replace('import { Type } from "typebox";', 'const Type = new Proxy({}, { get: () => (...args) => ({args}) });'),
);
const { installVisual } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);

function host() {
  let tool;
  installVisual({ registerTool: value => { tool = value; } });
  return { tool, run: (args, signal) => tool.execute('call', args, signal) };
}

test('pui_visual validates and returns inline visual details', async () => {
  const h = host();
  assert.equal(h.tool.name, 'pui_visual');
  assert.equal(h.tool.executionMode, 'sequential');
  const result = await h.run({ title: 'Map picker designs', html: '<div>Hello</div>', height: 420 });
  assert.match(result.content[0].text, /Map picker designs/);
  assert.deepEqual(result.details, { title: 'Map picker designs', height: 420, bytes: 16 });
});

test('pui_visual rejects empty, null-byte, oversized html and invalid height or title', async () => {
  const h = host();
  for (const bad of [
    { html: '' },
    { html: '   ' },
    { html: '<div>\u0000</div>' },
    { html: 'x'.repeat(262_145) },
    { html: '<div>ok</div>', title: '   ' },
    { html: '<div>ok</div>', title: 'bad\ntitle' },
    { html: '<div>ok</div>', height: 80 },
    { html: '<div>ok</div>', height: 1200 },
    { html: '<div>ok</div>', height: 240.5 },
  ]) {
    await assert.rejects(h.run(bad));
  }
});

test('pui_visual respects abort signal', async () => {
  const controller = new AbortController();
  controller.abort();
  await assert.rejects(host().run({ html: '<div>hi</div>' }, controller.signal), /abort/i);
});
