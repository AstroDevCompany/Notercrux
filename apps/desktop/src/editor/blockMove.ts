import { Extension } from '@tiptap/core';
import { Selection, type EditorState, type Transaction } from '@tiptap/pm/state';

const MOVABLE = new Set(['listItem', 'taskItem']);

/** Moves the block holding the cursor one position up or down among its siblings. */
export function moveBlock(
  state: EditorState,
  dispatch: ((tr: Transaction) => void) | undefined,
  dir: -1 | 1,
): boolean {
  const { $from } = state.selection;
  let depth = $from.depth;
  while (depth > 1 && !MOVABLE.has($from.node(depth).type.name)) depth--;
  if (depth < 1) return false;

  const parent = $from.node(depth - 1);
  const index = $from.index(depth - 1);
  const target = index + dir;
  if (target < 0 || target >= parent.childCount) return false;

  const start = $from.before(depth);
  const node = $from.node(depth);
  const end = start + node.nodeSize;
  if (!dispatch) return true;

  const offset = state.selection.from - start;
  const tr = state.tr;
  if (dir < 0) {
    const prevStart = start - parent.child(target).nodeSize;
    tr.delete(start, end).insert(prevStart, node);
    tr.setSelection(Selection.near(tr.doc.resolve(prevStart + offset)));
  } else {
    const nextEnd = end + parent.child(target).nodeSize;
    tr.insert(nextEnd, node).delete(start, end);
    const at = nextEnd - node.nodeSize + offset;
    tr.setSelection(Selection.near(tr.doc.resolve(at)));
  }
  dispatch(tr.scrollIntoView());
  return true;
}

export const BlockMove = Extension.create({
  name: 'blockMove',
  addKeyboardShortcuts() {
    // Dispatch straight to the view: these are plain ProseMirror transactions, not Tiptap commands.
    const run = (dir: -1 | 1) => () =>
      moveBlock(this.editor.state, (tr) => this.editor.view.dispatch(tr), dir);
    return {
      'Mod-Shift-ArrowUp': run(-1),
      'Mod-Shift-ArrowDown': run(1),
      'Alt-ArrowUp': run(-1),
      'Alt-ArrowDown': run(1),
    };
  },
});
