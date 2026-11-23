import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { MAX_SAVED_DRAFTS, parseDraftMap, upsertDraft } from '../src/utils/drafts.ts';

describe('offline per-chat draft persistence helpers', () => {
  it('saves, updates, and clears drafts cleanly', () => {
    let map = parseDraftMap(null);
    map = upsertDraft(map, '/tmp/a.jsonl', 'Half written prompt', 100);
    assert.equal(map['/tmp/a.jsonl']?.text, 'Half written prompt');

    const roundtrip = parseDraftMap(JSON.stringify(map));
    assert.equal(roundtrip['/tmp/a.jsonl']?.text, 'Half written prompt');

    map = upsertDraft(map, '/tmp/a.jsonl', '   ', 200);
    assert.equal(map['/tmp/a.jsonl'], undefined);
  });

  it('bounds saved drafts to MAX_SAVED_DRAFTS keeping the newest', () => {
    let map = parseDraftMap(null);
    for (let i = 0; i < MAX_SAVED_DRAFTS + 5; i += 1) {
      map = upsertDraft(map, `/tmp/s-${i}.jsonl`, `draft ${i}`, i * 10);
    }
    assert.equal(Object.keys(map).length, MAX_SAVED_DRAFTS);
    assert.equal(map['/tmp/s-0.jsonl'], undefined);
    assert.equal(map[`/tmp/s-${MAX_SAVED_DRAFTS + 4}.jsonl`]?.text, `draft ${MAX_SAVED_DRAFTS + 4}`);
  });
});
