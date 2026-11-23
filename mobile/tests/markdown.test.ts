import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  cleanUserDisplayText,
  isRtlText,
  parseInlineMarkdown,
  parseMarkdownBlocks,
} from '../src/utils/markdown.ts';

describe('mobile Markdown parser and bidi direction helper', () => {
  it('parses headings, lists, inline bold/code/links, quotes, tables, and fenced code', () => {
    const md = [
      '## Summary',
      '',
      '1. **Sessions Home** — uses `Today` and [docs](https://pi.dev).',
      '2. *Italic* and ~~strike~~ work too.',
      '',
      '- Bullet one',
      '- Bullet two',
      '',
      '> Quoted note',
      '',
      '| Col A | Col B |',
      '| --- | --- |',
      '| 1 | 2 |',
      '',
      '```ts',
      'const x = 1;',
      '```',
    ].join('\n');

    const blocks = parseMarkdownBlocks(md);
    assert.equal(blocks[0].type, 'heading');
    assert.equal(blocks[1].type, 'ordered');
    assert.equal(blocks[2].type, 'bullet');
    assert.equal(blocks[3].type, 'blockquote');
    assert.equal(blocks[4].type, 'table');
    assert.equal(blocks[5].type, 'code');

    const inline = parseInlineMarkdown('Use **bold** and `code` with [Pui](https://pi.dev)');
    assert.ok(inline.some((s) => s.type === 'bold' && s.text === 'bold'));
    assert.ok(inline.some((s) => s.type === 'code' && s.text === 'code'));
    const boldNested = parseInlineMarkdown(
      '**Single-line composer (`mobile/src/components/Composer.tsx`):**',
    );
    assert.ok(boldNested.some((s) => s.type === 'bold_code' && s.text === 'mobile/src/components/Composer.tsx'));
  });

  it('strips Pi internal image-coordinate annotations from user bubbles', () => {
    const raw =
      'Check these screenshots\n\n[Image: original 1080x2520, displayed at 857x2000. Multiply coordinates by 1.26 to map to original image.]\n[Image: original 1080x2520, displayed at 857x2000. Multiply coordinates by 1.26 to map to original image.]';
    assert.equal(cleanUserDisplayText(raw), 'Check these screenshots');
  });

  it('detects per-paragraph RTL vs LTR accurately', () => {
    assert.equal(isRtlText('Hello from Pui Mobile'), false);
    assert.equal(isRtlText('1. **Sessions Home** — Today'), false);
    assert.equal(isRtlText('הר געש עם לבה זורמת'), true);
    assert.equal(isRtlText('שלום Pui'), true);
  });
});
