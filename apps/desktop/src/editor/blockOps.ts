import type { Editor } from '@tiptap/core';
import type { Node as PMNode } from '@tiptap/pm/model';
import { NodeSelection, Selection, TextSelection } from '@tiptap/pm/state';
import { moveBlock } from './blockMove';

/**
 * Operations on "the block at a position", shared by the drag-handle menu, the right-click menu and
 * keyboard shortcuts.
 *
 * A *block* is the innermost list item containing the position, or else the top-level node. So a
 * list item is its own block (not the whole list), and everything inside a callout or quote belongs
 * to that callout or quote.
 */
export interface BlockRef {
  /** Position just before the block node. */
  pos: number;
  node: PMNode;
}

const LIST_ITEMS = new Set(['listItem', 'taskItem']);

export function blockAt(editor: Editor, at: number): BlockRef | null {
  const { doc } = editor.state;
  const clamped = Math.max(0, Math.min(at, doc.content.size));
  const $pos = doc.resolve(clamped);
  if ($pos.depth === 0) {
    const node = $pos.nodeAfter ?? $pos.nodeBefore;
    if (!node) return null;
    return { pos: $pos.nodeAfter ? clamped : clamped - node.nodeSize, node };
  }
  for (let d = $pos.depth; d >= 1; d--) {
    if (LIST_ITEMS.has($pos.node(d).type.name)) return { pos: $pos.before(d), node: $pos.node(d) };
  }
  return { pos: $pos.before(1), node: $pos.node(1) };
}

/** The block that starts exactly at `pos` (a position just before the block node). */
export function blockAtPos(editor: Editor, pos: number): BlockRef | null {
  const node = editor.state.doc.nodeAt(pos);
  return node ? { pos, node } : null;
}

/** The selected block if a block is selected, else the block holding the text cursor. */
export function currentBlock(editor: Editor): BlockRef | null {
  const { selection } = editor.state;
  if (selection instanceof NodeSelection) return { pos: selection.from, node: selection.node };
  return blockAt(editor, selection.from);
}

export function selectBlock(editor: Editor, pos: number) {
  const sel = NodeSelection.create(editor.state.doc, pos);
  editor.view.dispatch(editor.state.tr.setSelection(sel));
  editor.view.focus();
}

/** Puts the text cursor at the start of the block's first textblock. */
function enter(editor: Editor, pos: number) {
  const sel = Selection.near(editor.state.doc.resolve(pos + 1));
  editor.view.dispatch(editor.state.tr.setSelection(sel));
}

export type BlockKind =
  | 'paragraph'
  | 'h1'
  | 'h2'
  | 'h3'
  | 'bullet'
  | 'number'
  | 'todo'
  | 'quote'
  | 'callout'
  | 'code'
  | 'divider';

export const TURN_INTO: { kind: Exclude<BlockKind, 'divider'>; label: string }[] = [
  { kind: 'paragraph', label: 'Text' },
  { kind: 'h1', label: 'Heading 1' },
  { kind: 'h2', label: 'Heading 2' },
  { kind: 'h3', label: 'Heading 3' },
  { kind: 'bullet', label: 'Bulleted list' },
  { kind: 'number', label: 'Numbered list' },
  { kind: 'todo', label: 'To-do list' },
  { kind: 'quote', label: 'Quote' },
  { kind: 'callout', label: 'Callout' },
  { kind: 'code', label: 'Code block' },
];

/** What kind of block this is, for labels and "current choice" marks. */
export function kindOf(node: PMNode): BlockKind {
  switch (node.type.name) {
    case 'heading':
      return `h${Math.min(3, Math.max(1, Number(node.attrs.level ?? 1)))}` as BlockKind;
    case 'listItem': {
      return 'bullet'; // refined by the parent list type in `kindAt`
    }
    case 'taskItem':
      return 'todo';
    case 'blockquote':
      return 'quote';
    case 'callout':
      return 'callout';
    case 'codeBlock':
      return 'code';
    case 'horizontalRule':
      return 'divider';
    default:
      return 'paragraph';
  }
}

export function kindAt(editor: Editor, ref: BlockRef): BlockKind {
  if (ref.node.type.name === 'listItem') {
    const parent = editor.state.doc.resolve(ref.pos).parent;
    return parent.type.name === 'orderedList' ? 'number' : 'bullet';
  }
  return kindOf(ref.node);
}

export const kindLabel = (k: BlockKind) =>
  k === 'divider' ? 'Divider' : (TURN_INTO.find((t) => t.kind === k)?.label ?? 'Block');

/** Converts the block to another kind. Dividers cannot be converted. */
export function turnInto(
  editor: Editor,
  pos: number,
  target: Exclude<BlockKind, 'divider'>,
): boolean {
  const ref = blockAtPos(editor, pos);
  if (!ref || ref.node.type.name === 'horizontalRule') return false;
  if (kindAt(editor, ref) === target) return true;
  enter(editor, ref.pos);
  const chain = editor.chain().focus().clearNodes();
  switch (target) {
    case 'paragraph':
      return chain.setParagraph().run();
    case 'h1':
    case 'h2':
    case 'h3':
      return chain.setNode('heading', { level: Number(target[1]) }).run();
    case 'bullet':
      return chain.toggleBulletList().run();
    case 'number':
      return chain.toggleOrderedList().run();
    case 'todo':
      return chain.toggleTaskList().run();
    case 'quote':
      return chain.toggleBlockquote().run();
    case 'callout':
      return chain.toggleCallout().run();
    case 'code':
      return chain.setCodeBlock().run();
  }
}

/** Inserts a copy right after the block. Duplicate ids are repaired by the BlockId extension. */
export function duplicateBlock(editor: Editor, pos: number): boolean {
  const node = editor.state.doc.nodeAt(pos);
  if (!node) return false;
  const tr = editor.state.tr.insert(pos + node.nodeSize, node.copy(node.content));
  tr.setSelection(NodeSelection.create(tr.doc, pos + node.nodeSize));
  editor.view.dispatch(tr);
  return true;
}

export function deleteBlock(editor: Editor, pos: number): boolean {
  const node = editor.state.doc.nodeAt(pos);
  if (!node) return false;
  const tr = editor.state.tr.delete(pos, pos + node.nodeSize);
  tr.setSelection(Selection.near(tr.doc.resolve(Math.min(pos, tr.doc.content.size)), -1));
  editor.view.dispatch(tr);
  editor.view.focus();
  return true;
}

/** Moves the block one step among its siblings. Returns false at the first/last position. */
export function moveBlockBy(editor: Editor, pos: number, dir: -1 | 1): boolean {
  const ref = blockAtPos(editor, pos);
  if (!ref) return false;
  enter(editor, ref.pos);
  const moved = moveBlock(editor.state, (tr) => editor.view.dispatch(tr), dir);
  if (!moved) selectBlock(editor, ref.pos);
  return moved;
}

/** Can the block move in this direction? Used to disable menu entries. */
export function canMove(editor: Editor, pos: number, dir: -1 | 1): boolean {
  const ref = blockAtPos(editor, pos);
  if (!ref) return false;
  const $p = editor.state.doc.resolve(ref.pos);
  const index = $p.index();
  const target = index + dir;
  return target >= 0 && target < $p.parent.childCount;
}

/** Merges attributes into the block node itself (e.g. a code block's language). */
export function setBlockAttrs(
  editor: Editor,
  pos: number,
  attrs: Record<string, unknown>,
): boolean {
  const node = editor.state.doc.nodeAt(pos);
  if (!node) return false;
  const tr = editor.state.tr.setNodeMarkup(pos, undefined, { ...node.attrs, ...attrs });
  editor.view.dispatch(tr);
  return true;
}

/** Selects the block containing the text cursor (Esc). Returns false if already a block selection. */
export function selectBlockAtCursor(editor: Editor): boolean {
  const { selection } = editor.state;
  if (selection instanceof NodeSelection) return false;
  const ref = blockAt(editor, selection.from);
  if (!ref) return false;
  selectBlock(editor, ref.pos);
  return true;
}

/** Leaves a block selection by putting the cursor back in the block. */
export function leaveBlockSelection(editor: Editor): boolean {
  const { selection } = editor.state;
  if (!(selection instanceof NodeSelection)) return false;
  const sel = TextSelection.near(editor.state.doc.resolve(selection.from + 1));
  editor.view.dispatch(editor.state.tr.setSelection(sel));
  return true;
}
