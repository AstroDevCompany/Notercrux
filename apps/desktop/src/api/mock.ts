import {
  defaultSchema,
  newId,
  TITLE_PROPERTY,
  validateValue,
  isEmptyValue,
  type DatabaseSchema,
  type DbRecord,
  type Document,
  type Page,
} from '@notercrux/document-model';
import type { Api, SearchHit } from './types';

/**
 * localStorage-backed stand-in used when the UI runs in a plain browser (`pnpm web`) and in unit
 * tests, so the interface can be developed without the native shell. It mirrors the rules of the
 * Rust `storage` crate but is NOT the source of truth for behavior: the Rust crate is.
 */
const KEY = 'notercrux.mock.v1';

interface Data {
  pages: Page[];
  docs: Record<string, Document>;
  schemas: Record<string, DatabaseSchema>;
  /** pageId -> propertyId -> value */
  props: Record<string, Record<string, unknown>>;
}

function load(): Data {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const d = JSON.parse(raw) as Partial<Data>;
      return {
        pages: (d.pages ?? []).map((p) => ({ ...p, kind: p.kind ?? 'page' })),
        docs: d.docs ?? {},
        schemas: d.schemas ?? {},
        props: d.props ?? {},
      };
    }
  } catch {
    /* fall through to seed */
  }
  const now = Date.now();
  const mk = (
    title: string,
    parentId: string | null,
    position: number,
    icon: string | null = null,
  ): Page => ({
    id: newId(),
    parentId,
    kind: 'page',
    title,
    icon,
    position,
    createdAt: now,
    updatedAt: now,
    archivedAt: null,
  });
  const welcome = mk('Welcome to Notercrux', null, 0, '👋');
  const child = mk('A nested page', welcome.id, 0);
  const ideas = mk('Reading list', null, 1, '📚');
  return {
    pages: [welcome, child, ideas],
    docs: {
      [welcome.id]: {
        version: 1,
        blocks: [
          { id: newId(), type: 'heading', attrs: { level: 1 }, text: 'Welcome to Notercrux' },
          { id: newId(), type: 'paragraph', text: 'Press Cmd/Ctrl+K to open the command palette.' },
        ],
      },
    },
    schemas: {},
    props: {},
  };
}

const data = load();
const save = () => {
  try {
    localStorage.setItem(KEY, JSON.stringify(data));
  } catch {
    /* storage unavailable: keep working in memory */
  }
};
const siblings = (parentId: string | null) =>
  data.pages.filter((p) => p.parentId === parentId).sort((a, b) => a.position - b.position);
const renumber = (list: Page[]) => list.forEach((p, i) => (p.position = i));
const subtree = (id: string): string[] => [
  id,
  ...data.pages.filter((p) => p.parentId === id).flatMap((p) => subtree(p.id)),
];
const find = (id: string) => {
  const p = data.pages.find((x) => x.id === id);
  if (!p) throw new Error(`page ${id} not found`);
  return p;
};
const isDb = (id: string | null) =>
  !!id && data.pages.find((p) => p.id === id)?.kind === 'database';

function validateSchema(s: DatabaseSchema) {
  const ids = new Set<string>();
  for (const p of s.properties) {
    if (!p.name.trim()) throw new Error('property names must not be empty');
    if (p.id === TITLE_PROPERTY || ids.has(p.id)) throw new Error(`duplicate id “${p.id}”`);
    ids.add(p.id);
    if (p.type === 'relation' && !p.relationDatabase)
      throw new Error(`property “${p.name}” is a relation without a target database`);
  }
  if (!s.views.length) throw new Error('a database needs at least one view');
  for (const v of s.views) {
    for (const r of [
      ...v.sorts.map((x) => x.property),
      ...v.filters.map((x) => x.property),
      ...v.hidden,
    ]) {
      if (r !== TITLE_PROPERTY && !ids.has(r))
        throw new Error(`“${r}” refers to a property that does not exist`);
    }
  }
}

const linkTargets = (doc: Document | undefined): Set<string> => {
  const out = new Set<string>();
  const walk = (blocks: Document['blocks']) => {
    for (const b of blocks) {
      for (const m of b.marks ?? [])
        if (m.type === 'pageLink' && typeof m.attrs?.pageId === 'string') out.add(m.attrs.pageId);
      walk(b.children ?? []);
    }
  };
  walk(doc?.blocks ?? []);
  return out;
};

const bodyOf = (p: Page) => (data.docs[p.id]?.blocks ?? []).map((b) => b.text).join('\n');

function snippet(body: string, words: string[]): string {
  const lower = body.toLowerCase();
  const at = words
    .map((w) => lower.indexOf(w))
    .filter((i) => i >= 0)
    .sort((a, b) => a - b)[0];
  if (at === undefined) return body.slice(0, 90);
  const from = Math.max(0, at - 40);
  let out = body.slice(from, at + 90);
  for (const w of words)
    out = out.replace(
      new RegExp(w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi'),
      (m) => `\u0001${m}\u0002`,
    );
  return (from > 0 ? '…' : '') + out + (at + 90 < body.length ? '…' : '');
}

export const mockApi: Api = {
  async listPages() {
    return structuredClone(data.pages);
  },
  async createPage(parentId, title) {
    const now = Date.now();
    const page: Page = {
      id: newId(),
      parentId,
      kind: 'page',
      title,
      icon: null,
      position: siblings(parentId).length,
      createdAt: now,
      updatedAt: now,
      archivedAt: null,
    };
    data.pages.push(page);
    save();
    return structuredClone(page);
  },
  async renamePage(id, title) {
    Object.assign(find(id), { title, updatedAt: Date.now() });
    save();
  },
  async setPageIcon(id, icon) {
    find(id).icon = icon;
    save();
  },
  async movePage(id, newParentId, index) {
    if (newParentId && subtree(id).includes(newParentId)) {
      throw new Error('cannot move a page into itself or one of its descendants');
    }
    const page = find(id);
    const old = page.parentId;
    if (old !== newParentId && (isDb(old) || isDb(newParentId))) {
      throw new Error('Records cannot be moved into or out of a database.');
    }
    page.parentId = newParentId;
    const list = siblings(newParentId).filter((p) => p.id !== id);
    list.splice(Math.min(index, list.length), 0, page);
    renumber(list);
    if (old !== newParentId) renumber(siblings(old));
    save();
  },
  async duplicatePage(id) {
    const ids = subtree(id);
    const map = new Map(ids.map((i) => [i, newId()]));
    const now = Date.now();
    for (const old of ids) {
      const p = find(old);
      const nid = map.get(old)!;
      data.pages.push({
        ...p,
        id: nid,
        parentId: old === id ? p.parentId : map.get(p.parentId!)!,
        title: old === id ? `${p.title} (copy)` : p.title,
        position: old === id ? p.position + 1 : p.position,
        createdAt: now,
        updatedAt: now,
      });
      if (data.docs[old]) data.docs[nid] = structuredClone(data.docs[old]);
      if (data.schemas[old]) data.schemas[nid] = structuredClone(data.schemas[old]);
      if (data.props[old]) data.props[nid] = structuredClone(data.props[old]);
    }
    const copy = find(map.get(id)!);
    const list = siblings(copy.parentId).filter((p) => p.id !== copy.id);
    list.splice(list.findIndex((p) => p.id === id) + 1, 0, copy);
    renumber(list);
    save();
    return structuredClone(copy);
  },
  async archivePage(id) {
    for (const i of subtree(id)) find(i).archivedAt = Date.now();
    save();
  },
  async restorePage(id) {
    const page = find(id);
    const parent = page.parentId ? find(page.parentId) : null;
    if (parent?.archivedAt) {
      page.parentId = null;
      page.position = siblings(null).length;
    }
    for (const i of subtree(id)) find(i).archivedAt = null;
    save();
  },
  async deletePage(id) {
    const ids = new Set(subtree(id));
    data.pages = data.pages.filter((p) => !ids.has(p.id));
    for (const i of ids) {
      delete data.docs[i];
      delete data.schemas[i];
      delete data.props[i];
    }
    save();
  },
  async loadDocument(id) {
    return structuredClone(data.docs[id] ?? { version: 1, blocks: [] });
  },
  async saveDocument(id, doc) {
    data.docs[id] = structuredClone(doc);
    find(id).updatedAt = Date.now();
    save();
  },
  async searchPages(query): Promise<SearchHit[]> {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return [];
    return data.pages
      .filter((p) => {
        if (p.archivedAt) return false;
        const text = `${p.title} ${bodyOf(p)}`.toLowerCase();
        // Prefix match on word starts, like the FTS index.
        return words.every((w) =>
          new RegExp(`(^|[^\\p{L}\\p{N}])${w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`, 'u').test(
            text,
          ),
        );
      })
      .slice(0, 20)
      .map((page) => ({ page: structuredClone(page), snippet: snippet(bodyOf(page), words) }));
  },
  async backlinks(id) {
    return data.pages
      .filter((p) => !p.archivedAt && p.id !== id && linkTargets(data.docs[p.id]).has(id))
      .map((p) => structuredClone(p));
  },

  async createDatabase(parentId, title) {
    const page = await mockApi.createPage(parentId, title);
    find(page.id).kind = 'database';
    data.schemas[page.id] = defaultSchema();
    save();
    return structuredClone(find(page.id));
  },
  async getSchema(id) {
    const s = data.schemas[id];
    if (!s) throw new Error(`page ${id} is not a database`);
    return structuredClone(s);
  },
  async setSchema(id, schema) {
    const old = data.schemas[id];
    if (!old) throw new Error(`page ${id} is not a database`);
    validateSchema(schema);
    const records = data.pages.filter((p) => p.parentId === id);
    for (const op of old.properties) {
      const np = schema.properties.find((p) => p.id === op.id);
      const stale = !np || np.type !== op.type || np.relationDatabase !== op.relationDatabase;
      for (const r of records) {
        const vals = data.props[r.id];
        if (!vals || !(op.id in vals)) continue;
        if (stale) {
          delete vals[op.id];
        } else if (np.type === 'select' || np.type === 'multiSelect') {
          const keep = (x: unknown) => np.options?.some((o) => o.id === x);
          const v = vals[op.id];
          const cleaned = Array.isArray(v) ? v.filter(keep) : keep(v) ? v : null;
          if (isEmptyValue(cleaned)) delete vals[op.id];
          else vals[op.id] = cleaned;
        }
      }
    }
    data.schemas[id] = structuredClone(schema);
    save();
  },
  async listRecords(id): Promise<DbRecord[]> {
    if (!data.schemas[id]) throw new Error(`page ${id} is not a database`);
    return siblings(id)
      .filter((p) => !p.archivedAt)
      .map((p) => ({ page: structuredClone(p), values: structuredClone(data.props[p.id] ?? {}) }));
  },
  async setProperty(pageId, propertyId, value) {
    const page = find(pageId);
    const schema = page.parentId ? data.schemas[page.parentId] : undefined;
    if (!schema) throw new Error(`page ${pageId} is not a record of a database`);
    const prop = schema.properties.find((p) => p.id === propertyId);
    if (!prop) throw new Error(`“${propertyId}” refers to a property that does not exist`);
    const problem = validateValue(prop, value);
    if (problem) throw new Error(problem);
    const vals = (data.props[pageId] ??= {});
    if (isEmptyValue(value)) delete vals[propertyId];
    else vals[propertyId] = structuredClone(value);
    page.updatedAt = Date.now();
    save();
  },
  async getProperties(pageId) {
    return structuredClone(data.props[pageId] ?? {});
  },
};
