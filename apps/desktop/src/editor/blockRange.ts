import { Extension, type Editor } from '@tiptap/core';
import type { Node as PMNode } from '@tiptap/pm/model';
import { Plugin, PluginKey, Selection, TextSelection, type EditorState } from '@tiptap/pm/state';
import type { EditorView } from '@tiptap/pm/view';
import { turnInto, type BlockKind } from './blockOps';

/**
 * Multi-block selection ("block range"): a run of consecutive **top-level** blocks, made by
 * dragging a rubber band in the page margins. Lists, quotes and callouts count as one block.
 *
 * The range is tracked in plugin state (so it survives re-renders) and cleared by anything that
 * changes the document or the selection. Alongside it, the editor selection is set to the same text
 * span, which keeps native behaviours (focus, accessibility) working; copy, cut, delete and paste are
 * overridden here so whole blocks are handled, not a text span that would leave an empty block behind.
 */
export interface BlockRange {
  /** Position just before the first selected block. */
  from: number;
  /** Position just after the last selected block. */
  to: number;
}

export const blockRangeKey = new PluginKey<BlockRange | null>('blockRange');

export const getBlockRange = (state: EditorState): BlockRange | null =>
  blockRangeKey.getState(state) ?? null;

export interface RangeBlock {
  pos: number;
  node: PMNode;
}

/** The top-level blocks inside a range. */
export function blocksIn(state: EditorState, range: BlockRange): RangeBlock[] {
  const out: RangeBlock[] = [];
  state.doc.forEach((node, offset) => {
    if (offset >= range.from && offset + node.nodeSize <= range.to) out.push({ pos: offset, node });
  });
  return out;
}

/** Selects whole top-level blocks `first..last` (inclusive indexes). */
export function rangeOfIndexes(state: EditorState, first: number, last: number): BlockRange | null {
  const { doc } = state;
  if (doc.childCount === 0) return null;
  const a = Math.max(0, Math.min(first, last));
  const b = Math.min(doc.childCount - 1, Math.max(first, last));
  let from = 0;
  for (let i = 0; i < a; i++) from += doc.child(i).nodeSize;
  let to = from;
  for (let i = a; i <= b; i++) to += doc.child(i).nodeSize;
  return { from, to };
}

/**
 * Which top-level blocks does the vertical span [top, bottom] (client coordinates) touch? Blocks
 * are matched by their vertical extent only, so a drag in the margin selects the blocks beside it.
 */
export function pickBlocks(
  state: EditorState,
  top: number,
  bottom: number,
  rectOf: (pos: number) => { top: number; bottom: number } | null,
): BlockRange | null {
  let first = -1;
  let last = -1;
  let index = 0;
  state.doc.forEach((_node, offset) => {
    const r = rectOf(offset);
    if (r && r.bottom >= top && r.top <= bottom) {
      if (first < 0) first = index;
      last = index;
    }
    index++;
  });
  return first < 0 ? null : rangeOfIndexes(state, first, last);
}

export const sameRange = (a: BlockRange | null, b: BlockRange | null) =>
  a === b || (!!a && !!b && a.from === b.from && a.to === b.to);

/** Sets (or clears, with null) the block range and the matching editor selection. */
export function setBlockRange(editor: Editor, range: BlockRange | null) {
  const { state, view } = editor;
  if (sameRange(getBlockRange(state), range)) return;
  const tr = state.tr;
  if (range) {
    tr.setSelection(TextSelection.between(tr.doc.resolve(range.from), tr.doc.resolve(range.to), 1));
  }
  tr.setMeta(blockRangeKey, { range });
  view.dispatch(tr);
  if (range) view.focus();
}

function collapse(view: EditorView, pos: number, bias: 1 | -1) {
  const tr = view.state.tr.setSelection(Selection.near(view.state.doc.resolve(pos), bias));
  view.dispatch(tr);
}

/** Leaves the range, putting the cursor at its start (`'start'`) or end (`'end'`). */
export function clearBlockRange(editor: Editor, at: 'start' | 'end' = 'start'): boolean {
  const range = getBlockRange(editor.state);
  if (!range) return false;
  if (at === 'start') collapse(editor.view, range.from + 1, 1);
  else collapse(editor.view, range.to - 1, -1);
  return true;
}

/** Deletes the selected blocks. The document always keeps at least one (empty) block. */
export function deleteBlockRange(editor: Editor): boolean {
  const range = getBlockRange(editor.state);
  if (!range) return false;
  const { state, view } = editor;
  const tr = state.tr;
  const whole = range.from === 0 && range.to === state.doc.content.size;
  if (whole) tr.replaceWith(range.from, range.to, state.schema.nodes.paragraph!.create());
  else tr.delete(range.from, range.to);
  tr.setSelection(
    Selection.near(tr.doc.resolve(Math.min(range.from + (whole ? 1 : 0), tr.doc.content.size)), -1),
  );
  view.dispatch(tr);
  view.focus();
  return true;
}

/** Inserts a copy of the selected blocks after the range and selects the copy. */
export function duplicateBlockRange(editor: Editor): boolean {
  const range = getBlockRange(editor.state);
  if (!range) return false;
  const { state, view } = editor;
  const content = state.doc.slice(range.from, range.to).content;
  const tr = state.tr.insert(range.to, content);
  const next = { from: range.to, to: range.to + content.size };
  tr.setSelection(TextSelection.between(tr.doc.resolve(next.from), tr.doc.resolve(next.to), 1));
  tr.setMeta(blockRangeKey, { range: next });
  view.dispatch(tr);
  return true;
}

/** Converts every selected block to another kind (last to first, so earlier positions stay valid). */
export function turnRangeInto(editor: Editor, kind: Exclude<BlockKind, 'divider'>): boolean {
  const range = getBlockRange(editor.state);
  if (!range) return false;
  const blocks = blocksIn(editor.state, range);
  for (const b of [...blocks].reverse()) turnInto(editor, b.pos, kind);
  return true;
}

/** Writes the selected blocks to a clipboard event as HTML (lossless for the editor) and text. */
export function writeRangeToClipboard(view: EditorView, range: BlockRange, data: DataTransfer) {
  const slice = view.state.doc.slice(range.from, range.to);
  const { dom, text } = view.serializeForClipboard(slice);
  data.clearData();
  data.setData('text/html', dom.innerHTML);
  data.setData('text/plain', text);
}

/** Replaces the selected blocks with one empty paragraph and puts the cursor in it (before a paste). */
function replaceRangeWithParagraph(view: EditorView, range: BlockRange) {
  const para = view.state.schema.nodes.paragraph!.create();
  const tr = view.state.tr.replaceWith(range.from, range.to, para);
  tr.setSelection(Selection.near(tr.doc.resolve(range.from + 1), 1));
  view.dispatch(tr);
}

export const BlockRangeSelection = Extension.create({
  name: 'blockRange',
  // Must see Backspace/Delete/Escape before the editor's own handlers.
  priority: 1000,

  addProseMirrorPlugins() {
    const editor = this.editor;
    return [
      new Plugin<BlockRange | null>({
        key: blockRangeKey,
        state: {
          init: () => null,
          apply(tr, old) {
            const meta = tr.getMeta(blockRangeKey) as { range: BlockRange | null } | undefined;
            if (meta) return meta.range;
            if (!old) return null;
            // Appended transactions (id repair) only change attributes: the range still holds.
            if (tr.getMeta('appendedTransaction') !== undefined && !tr.selectionSet) return old;
            return tr.docChanged || tr.selectionSet ? null : old;
          },
        },
        props: {
          handleKeyDown(view, event) {
            const range = blockRangeKey.getState(view.state);
            if (!range) return false;
            const mod = event.metaKey || event.ctrlKey;
            const stop = () => {
              event.preventDefault();
              return true;
            };
            switch (event.key) {
              case 'Backspace':
              case 'Delete':
                return mod || event.altKey ? false : (deleteBlockRange(editor), stop());
              case 'Escape':
                return (clearBlockRange(editor, 'start'), stop());
              case 'Enter':
                return (clearBlockRange(editor, 'end'), stop());
              case 'ArrowUp':
              case 'ArrowLeft':
                if (event.altKey || mod || event.shiftKey) return event.altKey ? stop() : false;
                return (clearBlockRange(editor, 'start'), stop());
              case 'ArrowDown':
              case 'ArrowRight':
                if (event.altKey || mod || event.shiftKey) return event.altKey ? stop() : false;
                return (clearBlockRange(editor, 'end'), stop());
              case 'd':
                return mod ? (duplicateBlockRange(editor), stop()) : false;
              default:
                // Typing replaces nothing: leave the range and let the key act at the end of it.
                if (!mod && !event.altKey && event.key.length === 1) clearBlockRange(editor, 'end');
                return false;
            }
          },
          handleDOMEvents: {
            copy(view, event) {
              const range = blockRangeKey.getState(view.state);
              if (!range || !event.clipboardData) return false;
              event.preventDefault();
              writeRangeToClipboard(view, range, event.clipboardData);
              return true;
            },
            cut(view, event) {
              const range = blockRangeKey.getState(view.state);
              if (!range || !event.clipboardData) return false;
              event.preventDefault();
              writeRangeToClipboard(view, range, event.clipboardData);
              deleteBlockRange(editor);
              return true;
            },
            paste(view) {
              const range = blockRangeKey.getState(view.state);
              if (range) replaceRangeWithParagraph(view, range); // then the normal paste fills it
              return false;
            },
          },
        },
      }),
    ];
  },
});
