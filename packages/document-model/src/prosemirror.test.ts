import { describe, expect, it } from 'vitest';
import { fromProseMirror, toProseMirror, UnsupportedContentError } from './prosemirror';
import type { PMNode } from './types';

const p = (text: string, blockId?: string, marks?: PMNode['marks']): PMNode => ({
  type: 'paragraph',
  attrs: { blockId },
  content: [{ type: 'text', text, ...(marks && { marks }) }],
});

describe('ProseMirror <-> Notercrux document', () => {
  it('round-trips headings, lists, tasks, quotes, code and dividers with stable ids', () => {
    const pm: PMNode = {
      type: 'doc',
      content: [
        {
          type: 'heading',
          attrs: { blockId: 'h1', level: 2 },
          content: [{ type: 'text', text: 'Title' }],
        },
        {
          type: 'bulletList',
          content: [
            {
              type: 'listItem',
              attrs: { blockId: 'li1' },
              content: [
                { type: 'paragraph', content: [{ type: 'text', text: 'parent' }] },
                {
                  type: 'bulletList',
                  content: [
                    {
                      type: 'listItem',
                      attrs: { blockId: 'li2' },
                      content: [{ type: 'paragraph', content: [{ type: 'text', text: 'child' }] }],
                    },
                  ],
                },
              ],
            },
          ],
        },
        {
          type: 'taskList',
          content: [
            {
              type: 'taskItem',
              attrs: { blockId: 't1', checked: true },
              content: [{ type: 'paragraph', content: [{ type: 'text', text: 'done' }] }],
            },
          ],
        },
        {
          type: 'codeBlock',
          attrs: { blockId: 'c1', language: 'rust' },
          content: [{ type: 'text', text: 'fn main() {}' }],
        },
        { type: 'horizontalRule', attrs: { blockId: 'd1' } },
      ],
    };
    const doc = fromProseMirror(pm);
    expect(doc.blocks.map((b) => [b.id, b.type])).toEqual([
      ['h1', 'heading'],
      ['li1', 'bulletedListItem'],
      ['t1', 'todo'],
      ['c1', 'code'],
      ['d1', 'divider'],
    ]);
    expect(doc.blocks[1]!.children![0]!.id).toBe('li2');
    expect(doc.blocks[2]!.attrs).toEqual({ checked: true });
    expect(fromProseMirror(toProseMirror(doc))).toEqual(doc);
  });

  it('keeps inline marks as code-point ranges, including emoji', () => {
    const pm: PMNode = {
      type: 'doc',
      content: [
        {
          type: 'paragraph',
          attrs: { blockId: 'a' },
          content: [
            { type: 'text', text: '😀 plain ' },
            { type: 'text', text: 'bold', marks: [{ type: 'bold' }] },
            {
              type: 'text',
              text: ' link',
              marks: [{ type: 'link', attrs: { href: 'https://x.y' } }],
            },
          ],
        },
      ],
    };
    const doc = fromProseMirror(pm);
    const b = doc.blocks[0]!;
    expect(b.text).toBe('😀 plain bold link');
    expect(b.marks).toEqual([
      { from: 8, to: 12, type: 'bold' },
      { from: 12, to: 17, type: 'link', attrs: { href: 'https://x.y' } },
    ]);
    expect(fromProseMirror(toProseMirror(doc))).toEqual(doc);
  });

  it('merges adjacent text nodes that share a mark', () => {
    const bold = [{ type: 'bold' }];
    const pm: PMNode = {
      type: 'doc',
      content: [
        {
          type: 'paragraph',
          attrs: { blockId: 'a' },
          content: [
            { type: 'text', text: 'ab', marks: bold },
            { type: 'text', text: 'cd', marks: [...bold, { type: 'italic' }] },
          ],
        },
      ],
    };
    const marks = fromProseMirror(pm).blocks[0]!.marks!;
    expect(marks).toEqual([
      { from: 0, to: 4, type: 'bold' },
      { from: 2, to: 4, type: 'italic' },
    ]);
  });

  it('refuses unknown nodes and marks instead of dropping them', () => {
    expect(() => fromProseMirror({ type: 'doc', content: [{ type: 'mystery' }] })).toThrow(
      UnsupportedContentError,
    );
    expect(() =>
      fromProseMirror({ type: 'doc', content: [p('x', 'a', [{ type: 'sparkle' }])] }),
    ).toThrow(/mark "sparkle"/);
  });

  it('assigns ids to nodes that lack them, and an empty document still yields a paragraph', () => {
    const doc = fromProseMirror({ type: 'doc', content: [p('hi')] });
    expect(doc.blocks[0]!.id).toMatch(/^[0-9a-f-]{36}$/);
    expect(toProseMirror({ version: 1, blocks: [] }).content).toEqual([{ type: 'paragraph' }]);
  });

  it('keeps consecutive list items of one kind in a single list', () => {
    const doc = fromProseMirror({
      type: 'doc',
      content: [
        {
          type: 'orderedList',
          content: ['a', 'b'].map((t, i) => ({
            type: 'listItem',
            attrs: { blockId: `i${i}` },
            content: [{ type: 'paragraph', content: [{ type: 'text', text: t }] }],
          })),
        },
      ],
    });
    const pm = toProseMirror(doc);
    expect(pm.content).toHaveLength(1);
    expect(pm.content![0]!.content).toHaveLength(2);
  });
});

describe('callouts', () => {
  it('round-trips with icon and nested children', () => {
    const pm: PMNode = {
      type: 'doc',
      content: [
        {
          type: 'callout',
          attrs: { blockId: 'c1', icon: '🔥' },
          content: [
            { type: 'paragraph', content: [{ type: 'text', text: 'heads up' }] },
            {
              type: 'paragraph',
              attrs: { blockId: 'c2' },
              content: [{ type: 'text', text: 'detail' }],
            },
          ],
        },
      ],
    };
    const doc = fromProseMirror(pm);
    expect(doc.blocks[0]).toMatchObject({
      id: 'c1',
      type: 'callout',
      text: 'heads up',
      attrs: { icon: '🔥' },
    });
    expect(doc.blocks[0]!.children![0]).toMatchObject({ id: 'c2', text: 'detail' });
    expect(fromProseMirror(toProseMirror(doc))).toEqual(doc);
  });
});

describe('callout colour', () => {
  it('is stored as a "tone" attribute only when set, and round-trips', () => {
    const mk = (tone?: string): PMNode => ({
      type: 'doc',
      content: [
        {
          type: 'callout',
          attrs: { blockId: 'c', icon: '🔥', ...(tone ? { tone } : {}) },
          content: [{ type: 'paragraph', content: [{ type: 'text', text: 'x' }] }],
        },
      ],
    });
    expect(fromProseMirror(mk()).blocks[0]!.attrs).toEqual({ icon: '🔥' });
    const doc = fromProseMirror(mk('red'));
    expect(doc.blocks[0]!.attrs).toEqual({ icon: '🔥', tone: 'red' });
    expect(fromProseMirror(toProseMirror(doc))).toEqual(doc);
  });
});

describe('code block language', () => {
  it('round-trips the language, including ones the editor does not list', () => {
    const pm: PMNode = {
      type: 'doc',
      content: [
        {
          type: 'codeBlock',
          attrs: { blockId: 'k', language: 'elixir' },
          content: [{ type: 'text', text: 'IO.puts 1' }],
        },
      ],
    };
    const doc = fromProseMirror(pm);
    expect(doc.blocks[0]).toMatchObject({
      type: 'code',
      text: 'IO.puts 1',
      attrs: { language: 'elixir' },
    });
    expect(fromProseMirror(toProseMirror(doc))).toEqual(doc);
  });
});

describe('attachments', () => {
  const hash = 'a'.repeat(64);

  it('image and file blocks round-trip with their attachment reference and caption', () => {
    const pm: PMNode = {
      type: 'doc',
      content: [
        {
          type: 'image',
          attrs: {
            blockId: 'img1',
            attachment: hash,
            name: 'cat.png',
            mime: 'image/png',
            size: 1234,
            caption: 'A cat',
          },
        },
        {
          type: 'file',
          attrs: {
            blockId: 'f1',
            attachment: hash,
            name: 'plan.pdf',
            mime: 'application/pdf',
            size: 99,
          },
        },
      ],
    };
    const doc = fromProseMirror(pm);
    expect(doc.blocks[0]).toEqual({
      id: 'img1',
      type: 'image',
      text: 'A cat',
      attrs: { attachment: hash, name: 'cat.png', mime: 'image/png', size: 1234 },
    });
    expect(doc.blocks[1]).toEqual({
      id: 'f1',
      type: 'file',
      text: '',
      attrs: { attachment: hash, name: 'plan.pdf', mime: 'application/pdf', size: 99 },
    });
    expect(fromProseMirror(toProseMirror(doc))).toEqual(doc);
    expect(toProseMirror(doc).content?.[0]?.attrs?.caption).toBe('A cat');
  });
});
