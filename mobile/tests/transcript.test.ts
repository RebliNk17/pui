import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import type { MessageRecord, ToolRecord } from '../src/protocol/types.ts';
import { groupTranscript, summarizeActivity } from '../src/utils/transcript.ts';

function tool(name: string, extra: Partial<ToolRecord> = {}): ToolRecord {
  return {
    id: `${name}-1`,
    name,
    args: '',
    output: '',
    failed: false,
    finished: true,
    path: '',
    command: '',
    diff: '',
    added: 0,
    removed: 0,
    code: '',
    children: [],
    images: [],
    ...extra,
  };
}

function msg(role: string, text: string, tools: ToolRecord[] = []): MessageRecord {
  return {
    role,
    text,
    thinking: '',
    provider: 'anthropic',
    model: 'claude',
    tools,
    images: [],
    timestamp: 1000,
    cost: 0,
    effort: 'medium',
    revision: 1,
  };
}

describe('Pui Desktop-style grouped transcript activity', () => {
  it('collapses consecutive tool-calling assistant turns into one human-readable summary row', () => {
    const entries = groupTranscript([
      msg('user', 'Fix the bug'),
      msg('assistant', '', [tool('bash')]),
      msg('assistant', '', [tool('edit'), tool('edit')]),
      msg('assistant', '', [tool('bash'), tool('read')]),
      msg('assistant', 'All fixed!'),
    ]);

    assert.equal(entries.length, 3);
    assert.equal(entries[0].kind, 'user');
    assert.equal(entries[1].kind, 'activity');
    if (entries[1].kind === 'activity') {
      assert.equal(entries[1].tools.length, 5);
      assert.equal(entries[1].summary, 'read 1 file · ran 2 commands · edited 2 files');
    }
    assert.equal(entries[2].kind, 'assistant');
  });

  it('summarizes mixed tool counts cleanly', () => {
    const s = summarizeActivity([
      tool('read'),
      tool('read'),
      tool('bash'),
      tool('write'),
      tool('edit'),
    ]);
    assert.equal(s, 'read 2 files · ran 1 command · edited 2 files');
  });
});
