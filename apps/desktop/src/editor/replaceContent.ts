import type { Editor } from '@tiptap/core';
import { TextSelection } from '@tiptap/pm/state';
import { toProseMirror, type Document } from '@notercrux/document-model';

/** Structural equality of two documents (ids, text, marks, attributes, nesting). */
export function sameDocument(a: Document, b: Document): boolean {
  return JSON.stringify(a.blocks) === JSON.stringify(b.blocks);
}

/**
 * Swaps the editor's content for `doc` (changes that arrived from another device) without creating an
 * undo step or triggering an autosave, keeping the cursor near where it was.
 */
export function replaceContent(editor: Editor, doc: Document) {
  const node = editor.schema.nodeFromJSON(toProseMirror(doc));
  const { from } = editor.state.selection;
  let tr = editor.state.tr
    .replaceWith(0, editor.state.doc.content.size, node.content)
    .setMeta('addToHistory', false)
    .setMeta('preventUpdate', true);
  tr = tr.setSelection(TextSelection.near(tr.doc.resolve(Math.min(from, tr.doc.content.size))));
  editor.view.dispatch(tr);
}
