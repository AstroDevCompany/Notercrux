import { describe, expect, it } from 'vitest';
import {
  applyView,
  defaultSchema,
  displayValue,
  validateValue,
  type DatabaseSchema,
  type DbRecord,
  type Filter,
  type Property,
  type View,
} from './database';
import type { Page } from './types';

const page = (title: string): Page => ({
  id: title,
  parentId: 'db',
  kind: 'page',
  title,
  icon: null,
  position: 0,
  createdAt: 0,
  updatedAt: 0,
  archivedAt: null,
});
const rec = (title: string, values: Record<string, unknown> = {}): DbRecord => ({
  page: page(title),
  values,
});

const schema: DatabaseSchema = {
  ...defaultSchema(),
  properties: [
    {
      id: 'status',
      name: 'Status',
      type: 'select',
      options: [
        { id: 'todo', name: 'To do' },
        { id: 'done', name: 'Done' },
      ],
    },
    { id: 'points', name: 'Points', type: 'number' },
    { id: 'due', name: 'Due', type: 'date' },
    {
      id: 'tags',
      name: 'Tags',
      type: 'multiSelect',
      options: [
        { id: 'a', name: 'A' },
        { id: 'b', name: 'B' },
      ],
    },
    { id: 'done', name: 'Done', type: 'checkbox' },
    { id: 'notes', name: 'Notes', type: 'text' },
  ],
};
const view = (patch: Partial<View>): View => ({ ...schema.views[0]!, ...patch });
const titles = (rs: DbRecord[]) => rs.map((r) => r.page.title);

const data = [
  rec('Write docs', {
    status: 'todo',
    points: 3,
    due: '2026-03-01',
    tags: ['a'],
    notes: 'Docs for API',
  }),
  rec('Ship v1', { status: 'done', points: 8, due: '2026-02-01', tags: ['a', 'b'], done: true }),
  rec('Fix bug', { status: 'todo', points: 1 }),
  rec('apple pie', { points: 3, due: '2026-02-15', notes: '' }),
];

describe('validateValue (mirrors the Rust rules)', () => {
  const p = (type: Property['type']): Property => ({
    id: 'p',
    name: 'P',
    type,
    options: [{ id: 'a', name: 'A' }],
  });
  it('accepts matching values and empties; rejects mismatches', () => {
    expect(validateValue(p('number'), 1.5)).toBeNull();
    expect(validateValue(p('number'), '1')).toMatch(/expects a number/);
    expect(validateValue(p('date'), '2024-02-29')).toBeNull();
    expect(validateValue(p('date'), '2023-02-29')).toMatch(/date/);
    expect(validateValue(p('select'), 'zzz')).toMatch(/options/);
    expect(validateValue(p('multiSelect'), ['a'])).toBeNull();
    expect(validateValue(p('url'), 'nope')).toMatch(/link/);
    for (const t of ['text', 'number', 'date', 'select'] as const)
      expect(validateValue(p(t), null)).toBeNull();
  });
});

describe('applyView filters', () => {
  const f = (filters: Filter[], mode: View['filterMode'] = 'and') =>
    titles(applyView(data, schema, view({ filters, filterMode: mode })));

  it('filters by select, number, date, text, multi-select and checkbox', () => {
    expect(f([{ property: 'status', op: 'equals', value: 'todo' }])).toEqual([
      'Write docs',
      'Fix bug',
    ]);
    expect(f([{ property: 'status', op: 'notEquals', value: 'todo' }])).toEqual([
      'Ship v1',
      'apple pie',
    ]);
    expect(f([{ property: 'points', op: 'gte', value: 3 }])).toEqual([
      'Write docs',
      'Ship v1',
      'apple pie',
    ]);
    expect(f([{ property: 'points', op: 'lt', value: 3 }])).toEqual(['Fix bug']);
    expect(f([{ property: 'due', op: 'lt', value: '2026-02-20' }])).toEqual([
      'Ship v1',
      'apple pie',
    ]);
    expect(f([{ property: 'notes', op: 'contains', value: 'API' }])).toEqual(['Write docs']);
    expect(f([{ property: 'title', op: 'contains', value: 'APPLE' }])).toEqual(['apple pie']);
    expect(f([{ property: 'tags', op: 'contains', value: 'b' }])).toEqual(['Ship v1']);
    expect(f([{ property: 'done', op: 'equals', value: true }])).toEqual(['Ship v1']);
    expect(f([{ property: 'done', op: 'equals', value: false }])).toEqual([
      'Write docs',
      'Fix bug',
      'apple pie',
    ]);
  });

  it('handles empties, AND vs OR, and ignores filters on deleted properties', () => {
    expect(f([{ property: 'status', op: 'isEmpty' }])).toEqual(['apple pie']);
    expect(f([{ property: 'notes', op: 'isEmpty' }])).toEqual(['Ship v1', 'Fix bug', 'apple pie']);
    expect(f([{ property: 'tags', op: 'isNotEmpty' }])).toEqual(['Write docs', 'Ship v1']);
    const both: Filter[] = [
      { property: 'status', op: 'equals', value: 'todo' },
      { property: 'points', op: 'gt', value: 2 },
    ];
    expect(f(both, 'and')).toEqual(['Write docs']);
    expect(f(both, 'or')).toEqual(['Write docs', 'Ship v1', 'Fix bug', 'apple pie']);
    expect(f([{ property: 'deleted', op: 'equals', value: 'x' }])).toEqual(titles(data));
  });

  it('never matches empty cells for ordering comparisons', () => {
    expect(f([{ property: 'points', op: 'lt', value: 100 }])).toEqual(titles(data)); // all have points
    expect(f([{ property: 'due', op: 'gt', value: '2000-01-01' }])).toEqual([
      'Write docs',
      'Ship v1',
      'apple pie',
    ]);
  });
});

describe('applyView sorts', () => {
  const s = (sorts: View['sorts']) => titles(applyView(data, schema, view({ sorts })));

  it('sorts by title (case-insensitive, natural), number and date', () => {
    expect(s([{ property: 'title', direction: 'asc' }])).toEqual([
      'apple pie',
      'Fix bug',
      'Ship v1',
      'Write docs',
    ]);
    expect(s([{ property: 'title', direction: 'desc' }])).toEqual([
      'Write docs',
      'Ship v1',
      'Fix bug',
      'apple pie',
    ]);
    expect(s([{ property: 'points', direction: 'desc' }])).toEqual([
      'Ship v1',
      'Write docs',
      'apple pie',
      'Fix bug',
    ]);
    expect(s([{ property: 'due', direction: 'asc' }])).toEqual([
      'Ship v1',
      'apple pie',
      'Write docs',
      'Fix bug',
    ]);
  });

  it('keeps empty values last in both directions and breaks ties with later sorts, then original order', () => {
    expect(s([{ property: 'due', direction: 'desc' }]).slice(-1)).toEqual(['Fix bug']);
    expect(
      s([
        { property: 'points', direction: 'asc' },
        { property: 'title', direction: 'desc' },
      ]),
    ).toEqual(['Fix bug', 'Write docs', 'apple pie', 'Ship v1']);
    // Selects sort by option order (To do, Done), not alphabetically.
    expect(s([{ property: 'status', direction: 'asc' }])).toEqual([
      'Write docs',
      'Fix bug',
      'Ship v1',
      'apple pie',
    ]);
    expect(s([{ property: 'status', direction: 'desc' }])).toEqual([
      'Ship v1',
      'Write docs',
      'Fix bug',
      'apple pie',
    ]);
  });

  it('does not mutate its input and composes filter + sort', () => {
    const copy = structuredClone(data);
    const out = applyView(
      data,
      schema,
      view({
        filters: [{ property: 'status', op: 'equals', value: 'todo' }],
        sorts: [{ property: 'points', direction: 'asc' }],
      }),
    );
    expect(titles(out)).toEqual(['Fix bug', 'Write docs']);
    expect(data).toEqual(copy);
  });
});

describe('displayValue', () => {
  it('renders option names, lists and relation titles', () => {
    expect(displayValue(schema.properties[0]!, 'done')).toBe('Done');
    expect(displayValue(schema.properties[3]!, ['a', 'b'])).toBe('A, B');
    expect(
      displayValue({ id: 'r', name: 'R', type: 'relation' }, ['x', 'y'], (id) => id.toUpperCase()),
    ).toBe('X, Y');
    expect(displayValue(schema.properties[1]!, null)).toBe('');
  });
});
