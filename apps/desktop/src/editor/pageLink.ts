import { Extension, Mark, mergeAttributes } from '@tiptap/core';
import Suggestion, { type SuggestionProps } from '@tiptap/suggestion';
import { Plugin, PluginKey } from '@tiptap/pm/state';
import { useSyncExternalStore } from 'react';
import { actions, getState, live, pageLabel } from '../state/app';

/**
 * Inline link to another page, stored as a `pageLink` mark over the link text (the page title at
 * the time of insertion). Links are tracked by id, so renaming or moving the target never breaks
 * them; the visible text is a snapshot and is not rewritten on rename.
 */
export const PageLink = Mark.create({
  name: 'pageLink',
  inclusive: false,
  excludes: 'link',

  addAttributes() {
    return {
      pageId: {
        default: null,
        parseHTML: (el) => el.getAttribute('data-page-id'),
        renderHTML: (attrs) => ({ 'data-page-id': attrs.pageId as string }),
      },
    };
  },

  parseHTML() {
    return [{ tag: 'a[data-page-id]' }];
  },

  renderHTML({ HTMLAttributes }) {
    return ['a', mergeAttributes(HTMLAttributes, { class: 'page-link', href: '#' }), 0];
  },

  addProseMirrorPlugins() {
    return [
      new Plugin({
        key: new PluginKey('pageLinkClick'),
        props: {
          handleClick: (_view, _pos, event) => {
            const a = (event.target as HTMLElement | null)?.closest?.('a[data-page-id]');
            const id = a?.getAttribute('data-page-id');
            if (!id) return false;
            event.preventDefault();
            if (getState().pages.some((p) => p.id === id && !p.archivedAt)) actions.open(id);
            else actions.notify('That page was deleted or archived.');
            return true;
          },
        },
      }),
    ];
  },
});

// ---- `[[` picker --------------------------------------------------------------------------------

export interface LinkItem {
  id: string;
  title: string;
  hint?: string;
  icon: string | null;
  kind: 'page' | 'database';
  /** True for the "create a new page named …" entry. */
  create?: boolean;
}

export function linkItems(query: string, excludeId: string | null): LinkItem[] {
  const { pages } = getState();
  const q = query.trim().toLowerCase();
  const rank = (t: string) => (t.toLowerCase().startsWith(q) ? 0 : 1);
  const found = live(pages)
    .filter((p) => p.id !== excludeId && (!q || pageLabel(p).toLowerCase().includes(q)))
    .sort((a, b) => rank(pageLabel(a)) - rank(pageLabel(b)) || b.updatedAt - a.updatedAt)
    .slice(0, 8)
    .map<LinkItem>((p) => ({ id: p.id, title: pageLabel(p), icon: p.icon, kind: p.kind }));
  if (q && !found.some((i) => i.title.toLowerCase() === q)) {
    found.push({
      id: 'create',
      title: query.trim(),
      hint: 'Create new page',
      icon: null,
      kind: 'page',
      create: true,
    });
  }
  return found;
}

export interface LinkMenuState {
  open: boolean;
  items: LinkItem[];
  index: number;
  rect: { left: number; top: number; bottom: number } | null;
  select: (item: LinkItem) => void;
}

const CLOSED: LinkMenuState = { open: false, items: [], index: 0, rect: null, select: () => {} };
let state = CLOSED;
const listeners = new Set<() => void>();
const set = (s: LinkMenuState) => {
  state = s;
  listeners.forEach((l) => l());
};

export function useLinkMenu(): LinkMenuState {
  return useSyncExternalStore(
    (cb) => (listeners.add(cb), () => listeners.delete(cb)),
    () => state,
  );
}

const fromProps = (props: SuggestionProps<LinkItem>, index = 0): LinkMenuState => {
  const r = props.clientRect?.();
  return {
    open: true,
    items: props.items,
    index: Math.min(index, Math.max(props.items.length - 1, 0)),
    rect: r ? { left: r.left, top: r.top, bottom: r.bottom } : null,
    select: (item) => props.command(item),
  };
};

export const setLinkMenuIndex = (index: number) => set({ ...state, index });

export const PageLinkPicker = Extension.create<{ currentPageId: () => string | null }>({
  name: 'pageLinkPicker',

  addOptions() {
    return { currentPageId: () => null };
  },

  addProseMirrorPlugins() {
    const currentPageId = this.options.currentPageId;
    return [
      Suggestion<LinkItem, LinkItem>({
        editor: this.editor,
        pluginKey: new PluginKey('pageLinkPicker'),
        char: '[[',
        allowSpaces: true,
        startOfLine: false,
        allowedPrefixes: null,
        allow: ({ state: s }) => !s.selection.$from.parent.type.spec.code,
        items: ({ query }) => linkItems(query, currentPageId()),
        command: ({ editor, range, props }) => {
          const insert = (id: string, title: string) =>
            editor
              .chain()
              .focus()
              .insertContent([
                { type: 'text', text: title, marks: [{ type: 'pageLink', attrs: { pageId: id } }] },
                { type: 'text', text: ' ' },
              ])
              .run();
          editor.chain().focus().deleteRange(range).run();
          if (props.create) {
            void actions.createPage(null, { title: props.title, select: false }).then((p) => {
              if (p) insert(p.id, pageLabel(p));
            });
          } else {
            insert(props.id, props.title);
          }
        },
        render: () => ({
          onStart: (props) => set(fromProps(props)),
          onUpdate: (props) => set(fromProps(props, state.index)),
          onKeyDown: ({ event }) => {
            if (!state.open) return false;
            const n = state.items.length;
            if (event.key === 'ArrowDown') set({ ...state, index: n ? (state.index + 1) % n : 0 });
            else if (event.key === 'ArrowUp')
              set({ ...state, index: n ? (state.index - 1 + n) % n : 0 });
            else if (event.key === 'Enter' || event.key === 'Tab') {
              const item = state.items[state.index];
              if (!item) return false;
              state.select(item);
            } else return false;
            return true;
          },
          onExit: () => set(CLOSED),
        }),
      }),
    ];
  },
});
