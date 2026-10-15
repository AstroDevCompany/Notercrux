import { newId, type Document, type Page } from '@notercrux/document-model';
import type { Api } from './types';

/**
 * localStorage-backed stand-in used when the UI runs in a plain browser (`pnpm web`) so the
 * interface can be developed and tested without the native shell. Not a source of truth for
 * behavior: the Rust `storage` crate is.
 */
const KEY = 'notercrux.mock.v1';

interface Data {
  pages: Page[];
  docs: Record<string, Document>;
}

function load(): Data {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) return JSON.parse(raw) as Data;
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

export const mockApi: Api = {
  async listPages() {
    return structuredClone(data.pages);
  },
  async createPage(parentId, title) {
    const now = Date.now();
    const page: Page = {
      id: newId(),
      parentId,
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
      data.pages.push({
        ...p,
        id: map.get(old)!,
        parentId: old === id ? p.parentId : map.get(p.parentId!)!,
        title: old === id ? `${p.title} (copy)` : p.title,
        position: old === id ? p.position + 1 : p.position,
        createdAt: now,
        updatedAt: now,
      });
      if (data.docs[old]) data.docs[map.get(old)!] = structuredClone(data.docs[old]);
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
    for (const i of ids) delete data.docs[i];
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
  async searchPages(query) {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return [];
    const text = (p: Page) =>
      `${p.title} ${(data.docs[p.id]?.blocks ?? []).map((b) => b.text).join(' ')}`.toLowerCase();
    return data.pages
      .filter((p) => !p.archivedAt && words.every((w) => text(p).includes(w)))
      .slice(0, 20);
  },
};
