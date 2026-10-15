import { Extension } from '@tiptap/core';
import { NodeSelection } from '@tiptap/pm/state';
import { currentBlock, duplicateBlock, leaveBlockSelection, selectBlockAtCursor } from './blockOps';
import { isLinkMenuOpen } from './pageLink';
import { isSlashOpen } from './slashMenu';

/** Fired on the editor DOM when Enter is pressed on a selected block; the page opens its menu. */
export const BLOCK_MENU_EVENT = 'notercrux:block-menu';

/**
 * Keyboard access to block selection: Esc selects the block under the cursor (Esc again returns to
 * text), then Enter opens its options, Backspace/Delete removes it, Alt/⌘+Shift+arrows move it and ⌘/Ctrl+D duplicates it.
 */
export const BlockSelect = Extension.create({
  name: 'blockSelect',
  addKeyboardShortcuts() {
    return {
      Escape: ({ editor }) => {
        // The autocomplete menus use Escape to close themselves.
        if (isSlashOpen() || isLinkMenuOpen()) return false;
        return selectBlockAtCursor(editor) || leaveBlockSelection(editor);
      },
      Enter: ({ editor }) => {
        const { selection } = editor.state;
        if (!(selection instanceof NodeSelection)) return false;
        editor.view.dom.dispatchEvent(
          new CustomEvent(BLOCK_MENU_EVENT, { detail: { pos: selection.from } }),
        );
        return true;
      },
      'Mod-d': ({ editor }) => {
        const ref = currentBlock(editor);
        return ref ? duplicateBlock(editor, ref.pos) : false;
      },
    };
  },
});
