import { useSyncExternalStore } from 'react';
import {
  documentToMarkdown,
  markdownToDocument,
  pageFromJson,
  pageToJson,
  type Page,
} from '@notercrux/document-model';
import { api } from '../api';
import { pickTextFile, saveTextFile } from '../lib/files';
import { flushEditor } from './editorBridge';

export interface ConfirmRequest {
  title: string;
  message: string;
  confirmLabel: string;
  destructive?: boolean;
  onConfirm: () => void;
}

interface AppState {
  pages: Page[];
  currentId: string | null;
  /** Visited pages, oldest first, for back/forward. */
  history: { stack: string[]; index: number };
  expanded: Record<string, boolean>;
  sidebarOpen: boolean;
  paletteOpen: boolean;
  settingsOpen: boolean;
  confirm: ConfirmRequest | null;
  /** Page whose title is being edited in the sidebar. */
  renamingId: string | null;
  error: string | null;
  notice: { text: string; details: string[] } | null;
  loaded: boolean;
}

const EXPANDED_KEY = 'notercrux.expanded.v1';
const LAST_KEY = 'notercrux.lastPage.v1';

const safeGet = (k: string) => {
  try {
    return localStorage.getItem(k);
  } catch {
    return null;
  }
};
const safeSet = (k: string, v: string) => {
  try {
    localStorage.setItem(k, v);
  } catch {
    /* ignore */
  }
};

let state: AppState = {
  pages: [],
  currentId: null,
  history: { stack: [], index: -1 },
  expanded: JSON.parse(safeGet(EXPANDED_KEY) ?? '{}') as Record<string, boolean>,
  sidebarOpen: true,
  paletteOpen: false,
  settingsOpen: false,
  confirm: null,
  renamingId: null,
  error: null,
  notice: null,
  loaded: false,
};
const listeners = new Set<() => void>();

function set(patch: Partial<AppState>) {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
}

export function useApp<T>(select: (s: AppState) => T): T {
  return useSyncExternalStore(
    (cb) => (listeners.add(cb), () => listeners.delete(cb)),
    () => select(state),
  );
}
export const getState = () => state;

/** Runs a backend call, surfacing failures in the UI instead of swallowing them. */
async function guard<T>(fn: () => Promise<T>): Promise<T | undefined> {
  try {
    return await fn();
  } catch (e) {
    set({ error: String(e) });
    return undefined;
  }
}

export const live = (pages: Page[]) => pages.filter((p) => !p.archivedAt);
export const childrenOf = (pages: Page[], parentId: string | null) =>
  pages.filter((p) => p.parentId === parentId).sort((a, b) => a.position - b.position);

export const actions = {
  async init() {
    await guard(async () => {
      const pages = await api.listPages();
      const last = safeGet(LAST_KEY);
      const open =
        live(pages).find((p) => p.id === last) ?? childrenOf(live(pages), null)[0] ?? null;
      set({
        pages,
        currentId: open?.id ?? null,
        history: { stack: open ? [open.id] : [], index: open ? 0 : -1 },
        loaded: true,
      });
    });
  },
  async refresh() {
    const pages = await guard(() => api.listPages());
    if (!pages) return;
    const cur = pages.find((p) => p.id === state.currentId);
    set({ pages, currentId: cur && !cur.archivedAt ? cur.id : (live(pages)[0]?.id ?? null) });
  },
  /** Opens a page and records it in the navigation history. */
  select(id: string | null) {
    if (!id) return set({ currentId: null });
    safeSet(LAST_KEY, id);
    const { stack, index } = state.history;
    if (stack[index] === id) return set({ currentId: id });
    const next = [...stack.slice(0, index + 1), id].slice(-200);
    set({ currentId: id, history: { stack: next, index: next.length - 1 } });
  },
  /** Moves through history, skipping pages that no longer exist or are archived. */
  go(delta: -1 | 1) {
    const { stack, index } = state.history;
    const ok = (id: string | undefined) => !!id && live(state.pages).some((p) => p.id === id);
    let i = index + delta;
    while (i >= 0 && i < stack.length && !ok(stack[i])) i += delta;
    if (i < 0 || i >= stack.length) return;
    safeSet(LAST_KEY, stack[i]!);
    set({ currentId: stack[i]!, history: { stack, index: i } });
  },
  /** Expands every ancestor so `id` is visible in the sidebar tree. */
  reveal(id: string) {
    const expanded = { ...state.expanded };
    let cur = state.pages.find((x) => x.id === id);
    while (cur?.parentId) {
      expanded[cur.parentId] = true;
      const parentId: string = cur.parentId;
      cur = state.pages.find((x) => x.id === parentId);
    }
    safeSet(EXPANDED_KEY, JSON.stringify(expanded));
    set({ expanded });
  },
  /** Opens a page from anywhere (palette, links, breadcrumbs) and shows it in the tree. */
  open(id: string) {
    actions.reveal(id);
    actions.select(id);
  },
  toggleExpanded(id: string, value?: boolean) {
    const expanded = { ...state.expanded, [id]: value ?? !state.expanded[id] };
    safeSet(EXPANDED_KEY, JSON.stringify(expanded));
    set({ expanded });
  },
  async createPage(
    parentId: string | null = null,
    opts: { title?: string; select?: boolean } = {},
  ) {
    const page = await guard(() => api.createPage(parentId, opts.title ?? ''));
    if (!page) return;
    if (parentId) actions.toggleExpanded(parentId, true);
    await actions.refresh();
    if (opts.select !== false) actions.select(page.id);
    set({ renamingId: null });
    return page;
  },
  async createDatabase(parentId: string | null = null) {
    const page = await guard(() => api.createDatabase(parentId, 'Untitled database'));
    if (!page) return;
    if (parentId) actions.toggleExpanded(parentId, true);
    await actions.refresh();
    actions.select(page.id);
    return page;
  },
  async rename(id: string, title: string) {
    set({ renamingId: null });
    await guard(() => api.renamePage(id, title));
    await actions.refresh();
  },
  startRename: (id: string) => set({ renamingId: id }),
  cancelRename: () => set({ renamingId: null }),
  async setIcon(id: string, icon: string | null) {
    await guard(() => api.setPageIcon(id, icon));
    await actions.refresh();
  },
  async duplicate(id: string) {
    const copy = await guard(() => api.duplicatePage(id));
    await actions.refresh();
    if (copy) actions.select(copy.id);
  },
  async archive(id: string) {
    await guard(() => api.archivePage(id));
    await actions.refresh();
  },
  async restore(id: string) {
    await guard(() => api.restorePage(id));
    await actions.refresh();
  },
  async deletePermanently(id: string) {
    await guard(() => api.deletePage(id));
    await actions.refresh();
  },
  async move(id: string, parentId: string | null, index: number) {
    await guard(() => api.movePage(id, parentId, index));
    await actions.refresh();
  },
  /**
   * Moves a page next to (or inside) another. Index arithmetic uses all siblings, archived ones
   * included, because that is how the backend numbers positions.
   */
  async place(id: string, targetId: string, where: 'before' | 'after' | 'inside') {
    const { pages } = state;
    const target = pages.find((p) => p.id === targetId);
    if (!target || id === targetId) return;
    if (where === 'inside') {
      actions.toggleExpanded(targetId, true);
      const n = childrenOf(pages, targetId).filter((p) => p.id !== id).length;
      return actions.move(id, targetId, n);
    }
    const sibs = childrenOf(pages, target.parentId).filter((p) => p.id !== id);
    const at = sibs.findIndex((p) => p.id === targetId) + (where === 'after' ? 1 : 0);
    return actions.move(id, target.parentId, at);
  },
  /** Keyboard alternatives to drag-and-drop. */
  async shift(id: string, how: 'up' | 'down' | 'indent' | 'outdent') {
    const { pages } = state;
    const page = pages.find((p) => p.id === id);
    if (!page) return;
    const sibs = live(childrenOf(pages, page.parentId));
    const i = sibs.findIndex((p) => p.id === id);
    if (how === 'up' && i > 0) return actions.place(id, sibs[i - 1]!.id, 'before');
    if (how === 'down' && i < sibs.length - 1) return actions.place(id, sibs[i + 1]!.id, 'after');
    if (how === 'indent' && i > 0) return actions.place(id, sibs[i - 1]!.id, 'inside');
    if (how === 'outdent' && page.parentId) return actions.place(id, page.parentId, 'after');
  },
  async exportPage(id: string, format: 'md' | 'json') {
    await flushEditor();
    const page = state.pages.find((p) => p.id === id);
    if (!page) return;
    const doc = await guard(() => api.loadDocument(id));
    if (!doc) return;
    const title = pageLabel(page);
    const md = format === 'md' ? documentToMarkdown(page.title, doc) : null;
    const text = md ? md.value : pageToJson(page.title, page.icon, doc);
    const saved = await guard(() => saveTextFile(title, format, text));
    if (saved) {
      set({
        notice: {
          text: `Exported “${title}” as ${format === 'md' ? 'Markdown' : 'JSON'}.`,
          details: md?.warnings ?? [],
        },
      });
    }
  },
  async importFile() {
    const file = await guard(() => pickTextFile());
    if (!file) return;
    try {
      const isJson = /\.json$/i.test(file.name) || file.contents.trimStart().startsWith('{');
      let title: string;
      let icon: string | null = null;
      let document;
      let warnings: string[] = [];
      if (isJson) {
        const parsed = pageFromJson(file.contents);
        ({ title, icon, document } = parsed);
      } else {
        const md = markdownToDocument(file.contents);
        title = md.value.title ?? file.name.replace(/\.[^.]+$/, '');
        document = md.value.document;
        warnings = md.warnings;
      }
      const page = await api.createPage(null, title);
      if (icon) await api.setPageIcon(page.id, icon);
      await api.saveDocument(page.id, document);
      await actions.refresh();
      actions.select(page.id);
      set({
        notice: {
          text: `Imported “${title || 'Untitled'}”${warnings.length ? ` with ${warnings.length} note${warnings.length > 1 ? 's' : ''}` : ''}.`,
          details: warnings,
        },
      });
    } catch (e) {
      set({
        error: `Could not import ${file.name}: ${e instanceof Error ? e.message : String(e)}`,
      });
    }
  },
  dismissNotice: () => set({ notice: null }),
  notify: (text: string, details: string[] = []) => set({ notice: { text, details } }),
  setPalette: (paletteOpen: boolean) => set({ paletteOpen }),
  setSettings: (settingsOpen: boolean) => set({ settingsOpen }),
  toggleSidebar: () => set({ sidebarOpen: !state.sidebarOpen }),
  ask: (confirm: ConfirmRequest | null) => set({ confirm }),
  dismissError: () => set({ error: null }),
};

export const pageLabel = (p: Pick<Page, 'title'>) => p.title.trim() || 'Untitled';

export function breadcrumb(pages: Page[], id: string): Page[] {
  const trail: Page[] = [];
  let cur = pages.find((p) => p.id === id);
  while (cur) {
    trail.unshift(cur);
    const parentId: string | null = cur.parentId;
    cur = parentId ? pages.find((p) => p.id === parentId) : undefined;
  }
  return trail;
}

export const canGo = (s: Pick<AppState, 'history'>, delta: -1 | 1) =>
  delta < 0 ? s.history.index > 0 : s.history.index < s.history.stack.length - 1;
