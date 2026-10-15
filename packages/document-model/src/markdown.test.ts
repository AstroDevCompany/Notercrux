import { describe, expect, it } from 'vitest';
import { documentToMarkdown, markdownToDocument, pageFromJson, pageToJson } from './markdown';
import { fromProseMirror, toProseMirror } from './prosemirror';
import type { Block, Document } from './types';

/** Blocks without ids, for structural comparison across import/export. */
const strip = (blocks: Block[]): unknown[] =>
  blocks.map(({ id: _id, children, ...rest }) => ({
    ...rest,
    ...(children && { children: strip(children) }),
  }));

const b = (type: Block['type'], text: string, extra: Partial<Block> = {}): Block => ({
  id: 'x',
  type,
  text,
  ...extra,
});

describe('Markdown export', () => {
  it('renders every supported block, nesting and marks', () => {
    const doc: Document = {
      version: 1,
      blocks: [
        b('heading', 'Intro', { attrs: { level: 2 } }),
        b('paragraph', 'bold and code', {
          marks: [
            { from: 0, to: 4, type: 'bold' },
            { from: 9, to: 13, type: 'code' },
          ],
        }),
        b('bulletedListItem', 'one', {
          children: [b('todo', 'sub', { attrs: { checked: true } })],
        }),
        b('numberedListItem', 'first'),
        b('numberedListItem', 'second'),
        b('quote', 'wise words'),
        b('code', 'let x = 1;', { attrs: { language: 'ts' } }),
        b('divider', ''),
      ],
    };
    const { value, warnings } = documentToMarkdown('My page', doc);
    expect(warnings).toEqual([]);
    expect(value).toBe(
      [
        '# My page',
        '',
        '## Intro',
        '',
        '**bold** and `code`',
        '',
        '- one',
        '  - [x] sub',
        '',
        '1. first',
        '2. second',
        '',
        '> wise words',
        '',
        '```ts',
        'let x = 1;',
        '```',
        '',
        '---',
        '',
      ].join('\n'),
    );
  });

  it('warns about lossy conversions instead of hiding them', () => {
    const { warnings } = documentToMarkdown('', {
      version: 1,
      blocks: [
        b('callout', 'note', { attrs: { icon: '💡' } }),
        b('paragraph', 'u', { marks: [{ from: 0, to: 1, type: 'underline' }] }),
      ],
    });
    expect(warnings.join(' ')).toMatch(/Callouts/);
    expect(warnings.join(' ')).toMatch(/Underline/);
  });

  it('escapes text so it is not re-read as syntax', () => {
    const { value } = documentToMarkdown('', {
      version: 1,
      blocks: [b('paragraph', '# not a heading *or bold*')],
    });
    expect(value).toBe('\\# not a heading \\*or bold\\*\n');
    expect(markdownToDocument(value).value.document.blocks[0]).toMatchObject({
      type: 'paragraph',
      text: '# not a heading *or bold*',
    });
  });
});

describe('Markdown import', () => {
  it('parses headings, lists with nesting, tasks, quotes, code and rules; leading H1 becomes the title', () => {
    const md = [
      '# Title',
      '',
      'A *little* **bold** [link](https://x.y) and `code`.',
      '',
      '- a',
      '    - b',
      '- [x] done',
      '',
      '1. one',
      '2. two',
      '',
      '> quote',
      '> more',
      '',
      '~~~py',
      'print(1)',
      '~~~',
      '***',
    ].join('\n');
    const { value, warnings } = markdownToDocument(md);
    expect(warnings).toEqual([]);
    expect(value.title).toBe('Title');
    const blocks = strip(value.document.blocks) as Record<string, unknown>[];
    expect(blocks.map((x) => x.type)).toEqual([
      'paragraph',
      'bulletedListItem',
      'todo',
      'numberedListItem',
      'numberedListItem',
      'quote',
      'code',
      'divider',
    ]);
    expect(blocks[0]).toMatchObject({
      text: 'A little bold link and code.',
      marks: [
        { from: 2, to: 8, type: 'italic' },
        { from: 9, to: 13, type: 'bold' },
        { from: 14, to: 18, type: 'link', attrs: { href: 'https://x.y' } },
        { from: 23, to: 27, type: 'code' },
      ],
    });
    expect(blocks[1]).toMatchObject({
      text: 'a',
      children: [{ type: 'bulletedListItem', text: 'b' }],
    });
    expect(blocks[2]).toMatchObject({ attrs: { checked: true } });
    expect(blocks[5]).toMatchObject({ text: 'quote\nmore' });
    expect(blocks[6]).toMatchObject({ text: 'print(1)', attrs: { language: 'py' } });
  });

  it('reports what it cannot represent', () => {
    const { value, warnings } = markdownToDocument(
      [
        '| a | b |',
        '|---|---|',
        '| 1 | 2 |',
        '',
        '![alt](pic.png)',
        '',
        '<div>x</div>',
        '',
        '#### deep',
      ].join('\n'),
    );
    expect(warnings.join('\n')).toMatch(/Tables/);
    expect(warnings.join('\n')).toMatch(/Images/);
    expect(warnings.join('\n')).toMatch(/HTML/);
    expect(warnings.join('\n')).toMatch(/level 3/);
    expect(value.document.blocks.length).toBeGreaterThan(0); // nothing was dropped
  });

  it('round-trips an exported document, including through the editor representation', () => {
    const doc: Document = {
      version: 1,
      blocks: [
        b('heading', 'H', { attrs: { level: 1 } }),
        b('paragraph', 'x y z', {
          marks: [
            { from: 0, to: 1, type: 'bold' },
            { from: 2, to: 3, type: 'italic' },
          ],
        }),
        b('bulletedListItem', 'p', { children: [b('bulletedListItem', 'c')] }),
        b('todo', 't', { attrs: { checked: false } }),
        b('code', 'a\nb', { attrs: { language: 'rs' } }),
      ],
    };
    const md = documentToMarkdown('', doc).value;
    const back = markdownToDocument(md).value;
    // The leading H1 is promoted to the title; everything else must match exactly.
    expect(back.title).toBe('H');
    expect(strip(back.document.blocks)).toEqual(strip(doc.blocks.slice(1)));
    expect(fromProseMirror(toProseMirror(back.document)).blocks).toHaveLength(
      back.document.blocks.length,
    );
  });
});

describe('JSON export', () => {
  it('is lossless and rejects foreign or newer files', () => {
    const doc: Document = { version: 1, blocks: [b('callout', 'hi', { attrs: { icon: '🔥' } })] };
    const out = pageFromJson(pageToJson('T', '📚', doc));
    expect(out).toMatchObject({ title: 'T', icon: '📚', document: doc });
    expect(() => pageFromJson('{"format":"other"}')).toThrow(/not a Notercrux/);
    expect(() =>
      pageFromJson(
        JSON.stringify({
          format: 'notercrux-page',
          version: 1,
          document: { version: 99, blocks: [] },
        }),
      ),
    ).toThrow(/newer/);
  });
});

describe('page links', () => {
  it('survive a Markdown round trip and the editor conversion', () => {
    const id = '0190a000-0000-7000-8000-000000000001';
    const doc: Document = {
      version: 1,
      blocks: [
        b('paragraph', 'see Roadmap now', {
          marks: [{ from: 4, to: 11, type: 'pageLink', attrs: { pageId: id } }],
        }),
      ],
    };
    const md = documentToMarkdown('', doc);
    expect(md.warnings).toEqual([]);
    expect(md.value).toBe(`see [Roadmap](notercrux://page/${id}) now\n`);
    const back = markdownToDocument(md.value).value.document;
    expect(strip(back.blocks)).toEqual(strip(doc.blocks));
    expect(fromProseMirror(toProseMirror(back)).blocks[0]!.marks).toEqual(doc.blocks[0]!.marks);
  });
});
