import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { DatabaseSchema } from '@notercrux/document-model';

async function fresh() {
  localStorage.clear();
  vi.resetModules();
  return (await import('./mock')).mockApi;
}

describe('mock backend mirrors the database rules of the Rust crate', () => {
  let api: Awaited<ReturnType<typeof fresh>>;
  let db: string;
  let schema: DatabaseSchema;
  beforeEach(async () => {
    api = await fresh();
    db = (await api.createDatabase(null, 'Tasks')).id;
    schema = await api.getSchema(db);
    schema.properties = [
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
    ];
    await api.setSchema(db, schema);
  });

  it('validates values, lists records with values and clears empties', async () => {
    const r = await api.createPage(db, 'Write');
    await api.setProperty(r.id, 'points', 3);
    await api.setProperty(r.id, 'status', 'todo');
    await expect(api.setProperty(r.id, 'points', 'x')).rejects.toThrow(/number/);
    await expect(api.setProperty(r.id, 'status', 'nope')).rejects.toThrow(/options/);
    await expect(api.setProperty(r.id, 'ghost', 1)).rejects.toThrow(/does not exist/);
    expect((await api.listRecords(db))[0]!.values).toEqual({ points: 3, status: 'todo' });
    await api.setProperty(r.id, 'points', null);
    expect(await api.getProperties(r.id)).toEqual({ status: 'todo' });
  });

  it('cleans values on schema edits and refuses invalid schemas', async () => {
    const r = await api.createPage(db, 'x');
    await api.setProperty(r.id, 'status', 'done');
    await api.setProperty(r.id, 'points', 5);
    const next = structuredClone(schema);
    next.properties[0]!.options = [{ id: 'todo', name: 'To do' }]; // "done" removed
    next.properties[1]!.type = 'text'; // type change
    await api.setSchema(db, next);
    expect(await api.getProperties(r.id)).toEqual({});

    const bad = structuredClone(next);
    bad.views[0]!.sorts.push({ property: 'ghost', direction: 'asc' });
    await expect(api.setSchema(db, bad)).rejects.toThrow(/does not exist/);
    expect(await api.getSchema(db)).toEqual(next);
  });

  it('keeps records in their database and hides archived ones', async () => {
    const r = await api.createPage(db, 'r');
    const other = await api.createPage(null, 'other');
    await expect(api.movePage(r.id, other.id, 0)).rejects.toThrow(/database/);
    await expect(api.movePage(other.id, db, 0)).rejects.toThrow(/database/);
    await api.archivePage(r.id);
    expect(await api.listRecords(db)).toEqual([]);
  });

  it('computes backlinks from page-link marks', async () => {
    const target = await api.createPage(null, 'Target');
    const src = await api.createPage(null, 'Source');
    await api.saveDocument(src.id, {
      version: 1,
      blocks: [
        {
          id: 'b',
          type: 'paragraph',
          text: 'see Target',
          marks: [{ from: 4, to: 10, type: 'pageLink', attrs: { pageId: target.id } }],
        },
      ],
    });
    expect((await api.backlinks(target.id)).map((p) => p.id)).toEqual([src.id]);
    await api.saveDocument(src.id, { version: 1, blocks: [] });
    expect(await api.backlinks(target.id)).toEqual([]);
  });

  it('returns highlighted snippets from search', async () => {
    const p = await api.createPage(null, 'Notes');
    await api.saveDocument(p.id, {
      version: 1,
      blocks: [{ id: 'b', type: 'paragraph', text: 'the quick brown fox jumps over the lazy dog' }],
    });
    const hit = (await api.searchPages('lazy'))[0]!;
    expect(hit.page.id).toBe(p.id);
    expect(hit.snippet).toContain('\u0001lazy\u0002');
  });
});
