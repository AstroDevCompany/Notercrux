import { Extension, type Editor, type Range } from '@tiptap/core';
import Suggestion, { type SuggestionProps } from '@tiptap/suggestion';
import { PluginKey } from '@tiptap/pm/state';
import { useSyncExternalStore } from 'react';

export interface SlashItem {
  id: string;
  title: string;
  hint: string;
  keywords: string[];
  run: (editor: Editor, range: Range) => void;
}

const chain = (editor: Editor, range: Range) => editor.chain().focus().deleteRange(range);

export const SLASH_ITEMS: SlashItem[] = [
  {
    id: 'text',
    title: 'Text',
    hint: 'Plain paragraph',
    keywords: ['paragraph', 'p'],
    run: (e, r) => chain(e, r).setParagraph().run(),
  },
  {
    id: 'h1',
    title: 'Heading 1',
    hint: 'Large section title',
    keywords: ['title', 'h1', '#'],
    run: (e, r) => chain(e, r).setNode('heading', { level: 1 }).run(),
  },
  {
    id: 'h2',
    title: 'Heading 2',
    hint: 'Medium section title',
    keywords: ['h2', '##'],
    run: (e, r) => chain(e, r).setNode('heading', { level: 2 }).run(),
  },
  {
    id: 'h3',
    title: 'Heading 3',
    hint: 'Small section title',
    keywords: ['h3', '###'],
    run: (e, r) => chain(e, r).setNode('heading', { level: 3 }).run(),
  },
  {
    id: 'bullet',
    title: 'Bulleted list',
    hint: 'A simple list',
    keywords: ['ul', 'unordered', '-'],
    run: (e, r) => chain(e, r).toggleBulletList().run(),
  },
  {
    id: 'number',
    title: 'Numbered list',
    hint: 'An ordered list',
    keywords: ['ol', 'ordered', '1.'],
    run: (e, r) => chain(e, r).toggleOrderedList().run(),
  },
  {
    id: 'todo',
    title: 'To-do list',
    hint: 'Track tasks with checkboxes',
    keywords: ['todo', 'task', 'checkbox', 'check'],
    run: (e, r) => chain(e, r).toggleTaskList().run(),
  },
  {
    id: 'quote',
    title: 'Quote',
    hint: 'Capture a quotation',
    keywords: ['blockquote', '>'],
    run: (e, r) => chain(e, r).toggleBlockquote().run(),
  },
  {
    id: 'callout',
    title: 'Callout',
    hint: 'Make text stand out',
    keywords: ['note', 'info', 'highlight'],
    run: (e, r) => chain(e, r).toggleCallout().run(),
  },
  {
    id: 'code',
    title: 'Code block',
    hint: 'Monospaced code',
    keywords: ['pre', 'snippet', '```'],
    run: (e, r) => chain(e, r).toggleCodeBlock().run(),
  },
  {
    id: 'divider',
    title: 'Divider',
    hint: 'Visually separate sections',
    keywords: ['hr', 'rule', 'line', '---'],
    run: (e, r) => chain(e, r).setHorizontalRule().run(),
  },
];

export function filterSlashItems(query: string): SlashItem[] {
  const q = query.trim().toLowerCase();
  if (!q) return SLASH_ITEMS;
  const score = (i: SlashItem) =>
    i.title.toLowerCase().startsWith(q)
      ? 0
      : i.title.toLowerCase().includes(q)
        ? 1
        : i.keywords.some((k) => k.startsWith(q))
          ? 2
          : 3;
  return SLASH_ITEMS.filter((i) => score(i) < 3).sort((a, b) => score(a) - score(b));
}

// ---- bridge between the ProseMirror plugin and the React menu ------------------------------------

export interface SlashState {
  open: boolean;
  items: SlashItem[];
  index: number;
  rect: { left: number; top: number; bottom: number } | null;
  select: (item: SlashItem) => void;
}

const CLOSED: SlashState = { open: false, items: [], index: 0, rect: null, select: () => {} };
let state = CLOSED;
const listeners = new Set<() => void>();
const set = (s: SlashState) => {
  state = s;
  listeners.forEach((l) => l());
};

export function useSlashState(): SlashState {
  return useSyncExternalStore(
    (cb) => (listeners.add(cb), () => listeners.delete(cb)),
    () => state,
  );
}

function fromProps(props: SuggestionProps<SlashItem>, index = 0): SlashState {
  const r = props.clientRect?.();
  return {
    open: true,
    items: props.items,
    index: Math.min(index, Math.max(props.items.length - 1, 0)),
    rect: r ? { left: r.left, top: r.top, bottom: r.bottom } : null,
    select: (item) => props.command(item),
  };
}

export const SlashCommands = Extension.create({
  name: 'slashCommands',

  addProseMirrorPlugins() {
    return [
      Suggestion<SlashItem, SlashItem>({
        editor: this.editor,
        pluginKey: new PluginKey('slashCommands'),
        char: '/',
        allowSpaces: false,
        startOfLine: false,
        // Not inside code blocks, where "/" is just a character.
        allow: ({ state: s }) => !s.selection.$from.parent.type.spec.code,
        items: ({ query }) => filterSlashItems(query),
        command: ({ editor, range, props }) => props.run(editor, range),
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
