import type { Editor } from '@tiptap/core';
import { NodeSelection } from '@tiptap/pm/state';
import type { MenuItem } from '../components/ContextMenu';
import { actions, getState } from '../state/app';
import {
  currentBlock,
  canMove,
  deleteBlock,
  duplicateBlock,
  kindAt,
  moveBlockBy,
  turnInto,
  TURN_INTO,
} from './blockOps';

const mac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);
const MOD = mac ? '⌘' : 'Ctrl+';
const SHIFT = mac ? '⇧' : 'Shift+';
const ALT = mac ? '⌥' : 'Alt+';

/** Cut/copy go through the browser so rich content round-trips; they need focus and a user gesture. */
const exec = (editor: Editor, command: 'cut' | 'copy') => {
  editor.view.focus();
  document.execCommand(command);
};

async function paste(editor: Editor, plain: boolean) {
  try {
    if (!plain && typeof navigator.clipboard.read === 'function') {
      // Prefer rich content so formatting survives a paste from another app.
      for (const item of await navigator.clipboard.read()) {
        if (item.types.includes('text/html')) {
          const html = await (await item.getType('text/html')).text();
          editor.view.focus();
          editor.view.pasteHTML(html);
          return;
        }
      }
    }
    const text = await navigator.clipboard.readText();
    editor.view.focus();
    editor.view.pasteText(text);
  } catch {
    actions.notify(`The clipboard could not be read here. Use ${MOD}V to paste.`);
  }
}

export interface EditorMenuContext {
  /** Document position under the pointer. */
  pos: number;
  /** Page id if the pointer is over a page link. */
  linkPageId: string | null;
  openBlockMenu: (pos: number) => void;
}

/** Items for the right-click menu inside the page editor. */
export function buildEditorMenu(editor: Editor, ctx: EditorMenuContext): MenuItem[] {
  const items: MenuItem[] = [];
  const { selection } = editor.state;
  const hasRange = !selection.empty && !(selection instanceof NodeSelection);
  const block = currentBlock(editor);

  if (ctx.linkPageId) {
    const id = ctx.linkPageId;
    const live = getState().pages.some((p) => p.id === id && !p.archivedAt);
    items.push(
      {
        label: 'Open linked page',
        icon: 'link',
        disabled: !live,
        onSelect: () => actions.open(id),
      },
      {
        label: 'Remove link',
        icon: 'x',
        onSelect: () =>
          void editor
            .chain()
            .focus()
            .setTextSelection(ctx.pos)
            .extendMarkRange('pageLink')
            .unsetMark('pageLink')
            .run(),
      },
      { type: 'separator' },
    );
  }

  items.push(
    {
      label: 'Cut',
      shortcut: `${MOD}X`,
      disabled: selection.empty,
      onSelect: () => exec(editor, 'cut'),
    },
    {
      label: 'Copy',
      shortcut: `${MOD}C`,
      disabled: selection.empty,
      onSelect: () => exec(editor, 'copy'),
    },
    { label: 'Paste', shortcut: `${MOD}V`, onSelect: () => void paste(editor, false) },
    {
      label: 'Paste as plain text',
      shortcut: `${MOD}${SHIFT}V`,
      onSelect: () => void paste(editor, true),
    },
  );

  if (hasRange) {
    const toggle = (name: string, run: () => void, label: string, shortcut: string): MenuItem => ({
      label,
      shortcut,
      checked: editor.isActive(name),
      onSelect: run,
    });
    items.push(
      { type: 'separator' },
      { type: 'label', label: 'Format' },
      toggle('bold', () => void editor.chain().focus().toggleBold().run(), 'Bold', `${MOD}B`),
      toggle('italic', () => void editor.chain().focus().toggleItalic().run(), 'Italic', `${MOD}I`),
      toggle(
        'strike',
        () => void editor.chain().focus().toggleStrike().run(),
        'Strikethrough',
        `${MOD}${SHIFT}S`,
      ),
      toggle('code', () => void editor.chain().focus().toggleCode().run(), 'Code', `${MOD}E`),
    );
  }

  if (block) {
    const kind = kindAt(editor, block);
    items.push({ type: 'separator' });
    if (kind !== 'divider') {
      items.push({
        label: 'Turn into',
        children: TURN_INTO.map((t) => ({
          label: t.label,
          checked: t.kind === kind,
          onSelect: () => void turnInto(editor, block.pos, t.kind),
        })),
      });
    }
    items.push(
      { label: 'Block options…', onSelect: () => ctx.openBlockMenu(block.pos) },
      {
        label: 'Duplicate block',
        shortcut: `${MOD}D`,
        onSelect: () => void duplicateBlock(editor, block.pos),
      },
      {
        label: 'Move block up',
        shortcut: `${ALT}↑`,
        disabled: !canMove(editor, block.pos, -1),
        onSelect: () => void moveBlockBy(editor, block.pos, -1),
      },
      {
        label: 'Move block down',
        shortcut: `${ALT}↓`,
        disabled: !canMove(editor, block.pos, 1),
        onSelect: () => void moveBlockBy(editor, block.pos, 1),
      },
      { label: 'Delete block', danger: true, onSelect: () => void deleteBlock(editor, block.pos) },
    );
  }
  return items;
}
