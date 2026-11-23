import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  applyCompletion,
  detectCompletionToken,
  filterFileMentions,
  filterSlashCommands,
} from '../src/utils/completions.ts';

describe('composer / slash commands and @ file mention completions', () => {
  it('detects slash command only at the start of input', () => {
    assert.deepEqual(detectCompletionToken('/com', 4), {
      kind: 'command',
      start: 0,
      end: 4,
      query: 'com',
    });
    assert.equal(detectCompletionToken('check /compact later', 14), null);
  });

  it('detects @ file mentions anywhere in the draft', () => {
    const draft = 'Please inspect @src/rem and fix it';
    assert.deepEqual(detectCompletionToken(draft, 23), {
      kind: 'file',
      start: 15,
      end: 23,
      query: 'src/rem',
    });
  });

  it('applies slash command and @ file completions cleanly without duplicate spaces', () => {
    const cmdToken = detectCompletionToken('/co', 3)!;
    assert.deepEqual(applyCompletion('/co', cmdToken, 'compact'), {
      text: '/compact ',
      cursor: 9,
    });

    const draft = 'Check @src/m please';
    const fileToken = detectCompletionToken(draft, 12)!;
    assert.deepEqual(applyCompletion(draft, fileToken, 'src/main.rs'), {
      text: 'Check @src/main.rs please',
      cursor: 19,
    });
  });

  it('merges Pui local commands with Pi session commands and prioritizes prefix matches', () => {
    const filtered = filterSlashCommands(
      [{ name: 'review', description: 'Review current diff', source: 'pi' }],
      're',
    );
    assert.ok(filtered.some((c) => c.name === 'reload'));
    assert.ok(filtered.some((c) => c.name === 'review'));
    assert.equal(filtered[0].name, 'reload');
  });

  it('ranks filename prefix matches ahead of deep path substring matches', () => {
    const ranked = filterFileMentions(
      [
        { path: 'docs/archive/old_main_notes.md', name: 'old_main_notes.md' },
        { path: 'src/main.rs', name: 'main.rs' },
      ],
      'main',
    );
    assert.equal(ranked[0].path, 'src/main.rs');
  });
});
