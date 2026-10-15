import { describe, expect, it } from 'vitest';
import { applyView, defaultSchema, type DatabaseSchema, type DbRecord } from './database';
import { documentToMarkdown, markdownToDocument } from './markdown';
import { fromProseMirror, toProseMirror } from './prosemirror';
import type { Block, Document, Page } from './types';

/** Runs `fn`, prints its duration (visible with --reporter=verbose) and returns [result, ms]. */
function timed<T>(label: string, fn: () => T): [T, number] {
  const t = performance.now();
  const out = fn();
  const ms = performance.now() - t;
  console.log(`[scale] ${label}: ${ms.toFixed(1)} ms`);
  return [out, ms];
}

// Budgets are deliberately generous (several times the measured values) so the tests catch
// accidental algorithmic regressions without flaking on a slow CI machine.
describe('large databases', () => {
  const schema: DatabaseSchema = {
    ...defaultSchema(),
    properties: [
      {
        id: 'status',
        name: 'Status',
        type: 'select',
        options: [
          { id: 'a', name: 'A' },
          { id: 'b', name: 'B' },
          { id: 'c', name: 'C' },
        ],
      },
      { id: 'n', name: 'N', type: 'number' },
      {
        id: 'tags',
        name: 'Tags',
        type: 'multiSelect',
        options: [
          { id: 't1', name: 'T1' },
          { id: 't2', name: 'T2' },
        ],
      },
    ],
  };
  const page = (i: number): Page => ({
    id: String(i),
    parentId: 'db',
    kind: 'page',
    title: `Record ${(i * 7919) % 20000}`,
    icon: null,
    position: i,
    createdAt: 0,
    updatedAt: 0,
    archivedAt: null,
  });
  const records: DbRecord[] = Array.from({ length: 20_000 }, (_, i) => ({
    page: page(i),
    values: {
      status: ['a', 'b', 'c'][i % 3],
      n: (i * 31) % 1000,
      ...(i % 4 === 0 ? { tags: ['t1'] } : {}),
    },
  }));

  it('filters and sorts 20,000 records quickly and correctly', () => {
    const view = {
      ...schema.views[0]!,
      filters: [
        { property: 'status', op: 'equals' as const, value: 'a' },
        { property: 'n', op: 'gte' as const, value: 500 },
      ],
      sorts: [
        { property: 'n', direction: 'desc' as const },
        { property: 'title', direction: 'asc' as const },
      ],
    };
    const [out, ms] = timed('filter+sort 20k records', () => applyView(records, schema, view));
    expect(ms).toBeLessThan(1500);
    expect(out.length).toBeGreaterThan(0);
    expect(out.every((r) => r.values.status === 'a' && (r.values.n as number) >= 500)).toBe(true);
    for (let i = 1; i < out.length; i++) {
      const [a, b] = [out[i - 1]!, out[i]!];
      expect((a.values.n as number) >= (b.values.n as number)).toBe(true);
    }
  });

  it('sorting by title on 20,000 records is quick', () => {
    const [, ms] = timed('sort by title 20k', () =>
      applyView(records, schema, {
        ...schema.views[0]!,
        sorts: [{ property: 'title', direction: 'asc' }],
      }),
    );
    expect(ms).toBeLessThan(2000);
  });
});

describe('long documents', () => {
  const blocks = (n: number): Block[] =>
    Array.from({ length: n }, (_, i) => ({
      id: `b${i}`,
      type: i % 7 === 0 ? 'heading' : i % 5 === 0 ? 'bulletedListItem' : 'paragraph',
      ...(i % 7 === 0 ? { attrs: { level: 2 } } : {}),
      text: `Block ${i}: the quick brown fox jumps over the lazy dog.`,
      marks: [{ from: 0, to: 5, type: 'bold' }],
    }));
  const doc: Document = { version: 1, blocks: blocks(10_000) };

  it('converts a 10,000-block document to the editor model and back without loss', () => {
    const [pm, t1] = timed('document → ProseMirror (10k blocks)', () => toProseMirror(doc));
    const [back, t2] = timed('ProseMirror → document (10k blocks)', () => fromProseMirror(pm));
    expect(t1 + t2).toBeLessThan(3000);
    expect(back.blocks).toHaveLength(10_000);
    expect(back.blocks[9_999]).toEqual(doc.blocks[9_999]);
    expect(back.blocks[7]!.marks).toEqual(doc.blocks[7]!.marks);
  });

  it('exports and re-imports 10,000 blocks as Markdown', () => {
    const [md, t1] = timed('document → Markdown (10k blocks)', () =>
      documentToMarkdown('Big', doc),
    );
    const [imp, t2] = timed('Markdown → document (10k blocks)', () => markdownToDocument(md.value));
    expect(t1 + t2).toBeLessThan(4000);
    expect(imp.value.title).toBe('Big');
    expect(imp.value.document.blocks).toHaveLength(10_000);
    expect(imp.warnings).toEqual([]);
  });
});
