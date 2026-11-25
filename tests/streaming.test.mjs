import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

// No provider traffic: use the installed Pi event encoder with a deliberately
// shallow seed, then replay its exact wire records through the Rust reducer.
test('current Pi JSON event encoding replays without transient doubled words', async t => {
  const sdk = process.env.PUI_PI_SDK || join(process.env.HOME, '.pi/agent/install/releases/1.5.0/node_modules/@earendil-works/pi-coding-agent/dist/index.js');
  const encoder = new URL('./modes/json-event.js', pathToFileURL(sdk));
  try { await readFile(encoder); } catch { t.skip('Installed Pi SDK encoder is unavailable'); return; }
  const { toJsonEvent } = await import(encoder.href);
  const partial = { role: 'assistant', content: [{ type: 'text', text: 'Confirmed' }] };
  const records = [toJsonEvent({ type: 'message_start', message: partial })];
  for (const event of [
    { type: 'text_start', contentIndex: 0 },
    { type: 'text_delta', contentIndex: 0, delta: 'Confirmed' },
    { type: 'text_delta', contentIndex: 0, delta: ': the running app' },
    { type: 'text_end', contentIndex: 0, content: 'Confirmed: the running app' },
  ]) {
    const encoded = toJsonEvent({ type: 'message_update', message: partial, assistantMessageEvent: { ...event, partial } });
    assert.equal(encoded.message, undefined);
    assert.equal(encoded.assistantMessageEvent.partial, undefined);
    records.push(encoded);
  }
  const dir = await mkdtemp(join(tmpdir(), 'pui-wire-replay-'));
  const file = join(dir, 'events.json');
  await writeFile(file, JSON.stringify(records));
  // Fallback only: a caller's cargo (CI pins 1.88) must win, or a different
  // compiler would rebuild the whole crate (~5 minutes on CI runners).
  const toolchain = join(process.env.HOME, '.rustup/toolchains/stable-aarch64-apple-darwin/bin');
  const env = { ...process.env, PUI_STREAM_FIXTURE: file, PATH: `${process.env.PATH}:${toolchain}` };
  delete env.CARGO_HTTP_PROXY_CAINFO;
  const replay = spawnSync('cargo', ['test', '--offline', '--bin', 'pui', 'replay_current_pi_wire_fixture'], { env, encoding: 'utf8' });
  assert.equal(replay.status, 0, `${replay.stdout}\n${replay.stderr}`);
  assert.match(replay.stdout, /1 passed/);
});
