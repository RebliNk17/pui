import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import type { TranscriptEntry } from '../src/utils/transcript.ts';
import { findMatchingEntryIndices } from '../src/utils/search.ts';

describe('in-chat transcript search', () => {
  it('finds matching user, assistant, and activity entries case-insensitively', () => {
    const entries: TranscriptEntry[] = [
      {
        kind: 'user',
        key: 'u1',
        message: {
          role: 'user',
          text: 'Fix the Tailscale pairing flow',
          thinking: '',
          provider: '',
          model: '',
          tools: [],
          images: [],
          timestamp: 1,
          cost: 0,
          effort: '',
          revision: 1,
        },
      },
      {
        kind: 'activity',
        key: 'a1',
        summary: 'read 2 files · ran 1 command',
        thinking: ['Inspecting websocket handshake'],
        tools: [],
        hasRunning: false,
        hasError: false,
      },
      {
        kind: 'assistant',
        key: 'as1',
        inlineTools: [],
        message: {
          role: 'assistant',
          text: 'Tailscale WebSocket pairing is fixed.',
          thinking: '',
          provider: 'anthropic',
          model: 'claude',
          tools: [],
          images: [],
          timestamp: 2,
          cost: 0,
          effort: '',
          revision: 2,
        },
      },
    ];

    assert.deepEqual(findMatchingEntryIndices(entries, 'tailscale'), [0, 2]);
    assert.deepEqual(findMatchingEntryIndices(entries, 'handshake'), [1]);
    assert.deepEqual(findMatchingEntryIndices(entries, ''), []);
    assert.deepEqual(findMatchingEntryIndices(entries, 'nonexistent'), []);
  });
});
