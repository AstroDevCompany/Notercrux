import { describe, expect, it } from 'vitest';
import { Editor } from '@tiptap/core';
import { NodeSelection } from '@tiptap/pm/state';
import { fromProseMirror } from '@notercrux/document-model';
import {
  blockAt,
  canMove,
  deleteBlock,
  duplicateBlock,
  kindAt,
  leaveBlockSelection,
  moveBlockBy,
  selectBlockAtCursor,
  setBlockAttrs,
  turnInto,
} from './blockOps';
import { BLOCK_MENU_EVENT } from './blockSelect';
import { createExtensions } from './extensions';

const p = (text: string) => ({ type: 'paragraph', content: [{ type: 'text', text }] });
const li = (text: string, kids: unknown[] = []) => ({
  type: 'listItem',
  content: [p(text), ...kids],
});
const make = (content: unknown[]) =>
  new Editor({
    extensions: createExtensions(),
    content: { type: 'doc', content } as never,
    element: document.createElement('div'),
  });
const blocks = (e: Editor) => fromProseMirror(e.getJSON() as never).blocks;
const texts = (e: Editor) =>
  blocks(e)
    .map((b) => b.text)
    .filter(Boolean);
/** Position of the block whose first text is `text`. */
const find = (e: Editor, text: string) => {
  let at = -1;
  e.state.doc.descendants((n, pos) => {
    if (n.isText && n.text === text) at = pos;
  });
  return blockAt(e, at)!;
};
const press = (e: Editor, key: string, init: KeyboardEventInit = {}) =>
  e.view.dom.dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }),
  );

describe('which block a position belongs to', () => {
  it('is the list item for list content, the container for callouts, the node for plain blocks', () => {
    const e = make([
      p('plain'),
      {
        type: 'bulletList',
        content: [li('item', [{ type: 'bulletList', content: [li('child')] }])],
      },
      { type: 'callout', attrs: { icon: '💡' }, content: [p('head'), p('body')] },
    ]);
    expect(find(e, 'plain').node.type.name).toBe('paragraph');
    expect(find(e, 'item').node.type.name).toBe('listItem');
    expect(find(e, 'child').node.type.name).toBe('listItem');
    expect(find(e, 'child').pos).not.toBe(find(e, 'item').pos);
    expect(find(e, 'body').node.type.name).toBe('callout'); // nested paragraph belongs to the callout
    e.destroy();
  });
});

describe('turning a block into another kind', () => {
  it.each([
    ['h2', 'heading'],
    ['bullet', 'listItem'],
    ['todo', 'taskItem'],
    ['quote', 'blockquote'],
    ['callout', 'callout'],
    ['code', 'codeBlock'],
  ] as const)('paragraph → %s', (target, nodeName) => {
    const e = make([p('one'), p('two'), p('three')]);
    const ref = find(e, 'two');
    expect(turnInto(e, ref.pos, target)).toBe(true);
    expect(texts(e)).toEqual(['one', 'two', 'three']); // content is never lost
    const after = find(e, 'two');
    expect(after.node.type.name).toBe(nodeName);
    expect(kindAt(e, after)).toBe(target);
    e.destroy();
  });

  it('converts back to text and between list types, leaving neighbours alone', () => {
    const e = make([
      { type: 'heading', attrs: { level: 1 }, content: [{ type: 'text', text: 'Title' }] },
      p('after'),
    ]);
    turnInto(e, find(e, 'Title').pos, 'paragraph');
    expect(blocks(e).map((b) => b.type)).toEqual(['paragraph', 'paragraph']);

    const l = make([{ type: 'bulletList', content: [li('a'), li('b')] }]);
    turnInto(l, find(l, 'a').pos, 'number');
    expect(texts(l)).toEqual(['a', 'b']);
    expect(blocks(l).find((b) => b.text === 'a')!.type).toBe('numberedListItem');
    e.destroy();
    l.destroy();
  });

  it('refuses to convert a divider', () => {
    const e = make([p('x'), { type: 'horizontalRule' }]);
    const hr = blockAt(e, e.state.doc.content.size - 1)!;
    expect(hr.node.type.name).toBe('horizontalRule');
    expect(turnInto(e, hr.pos, 'quote')).toBe(false);
    e.destroy();
  });
});

describe('block actions', () => {
  it('duplicates a block with fresh ids, selects the copy, and keeps list item children', () => {
    const e = make([
      {
        type: 'bulletList',
        content: [li('parent', [{ type: 'bulletList', content: [li('child')] }])],
      },
    ]);
    duplicateBlock(e, find(e, 'parent').pos);
    const out = blocks(e);
    expect(out.slice(0, 2).map((b) => b.text)).toEqual(['parent', 'parent']);
    expect(out[1]!.children![0]!.text).toBe('child');
    expect(new Set(out.slice(0, 2).map((b) => b.id)).size).toBe(2); // BlockId repaired the duplicate
    expect(e.state.selection).toBeInstanceOf(NodeSelection);
    e.destroy();
  });

  it('deletes a block, including the last one in the document', () => {
    const e = make([p('keep'), p('drop')]);
    deleteBlock(e, find(e, 'drop').pos);
    expect(texts(e)).toEqual(['keep']);
    const single = make([p('only')]);
    deleteBlock(single, find(single, 'only').pos);
    expect(single.state.doc.childCount).toBe(1); // the document always keeps one (empty) block
    e.destroy();
    single.destroy();
  });

  it('moves a block and reports whether movement is possible', () => {
    const e = make([p('a'), p('b'), p('c')]);
    expect(canMove(e, find(e, 'a').pos, -1)).toBe(false);
    expect(canMove(e, find(e, 'a').pos, 1)).toBe(true);
    expect(moveBlockBy(e, find(e, 'b').pos, -1)).toBe(true);
    expect(texts(e)).toEqual(['b', 'a', 'c']);
    expect(moveBlockBy(e, find(e, 'b').pos, -1)).toBe(false);
    expect(texts(e)).toEqual(['b', 'a', 'c']);
    e.destroy();
  });

  it('edits block properties: code language, callout icon and colour', () => {
    const e = make([
      {
        type: 'codeBlock',
        attrs: { language: null },
        content: [{ type: 'text', text: 'fn main() {}' }],
      },
      { type: 'callout', attrs: { icon: '💡' }, content: [p('note')] },
    ]);
    setBlockAttrs(e, find(e, 'fn main() {}').pos, { language: 'rust' });
    setBlockAttrs(e, find(e, 'note').pos, { icon: '🚀', tone: 'red' });
    const [code, callout] = blocks(e);
    expect(code!.attrs).toEqual({ language: 'rust' });
    expect(callout!.attrs).toEqual({ icon: '🚀', tone: 'red' });
    e.destroy();
  });
});

describe('selecting blocks from the keyboard', () => {
  it('Esc selects the block under the cursor, Esc again returns to text, Delete removes the block', () => {
    const e = make([p('one'), p('two'), p('three')]);
    e.commands.setTextSelection(find(e, 'two').pos + 2);
    press(e, 'Escape');
    expect(e.state.selection).toBeInstanceOf(NodeSelection);
    expect((e.state.selection as NodeSelection).node.textContent).toBe('two');
    press(e, 'Escape');
    expect(e.state.selection).not.toBeInstanceOf(NodeSelection);
    expect(e.state.selection.$from.parent.textContent).toBe('two');

    expect(selectBlockAtCursor(e)).toBe(true);
    expect(leaveBlockSelection(e)).toBe(true);
    selectBlockAtCursor(e);
    e.commands.deleteSelection();
    expect(texts(e)).toEqual(['one', 'three']);
    e.destroy();
  });

  it('Enter on a selected block asks the page to open its menu; ⌘/Ctrl+D duplicates', () => {
    const e = make([p('one'), p('two')]);
    const events: number[] = [];
    e.view.dom.addEventListener(BLOCK_MENU_EVENT, (ev) =>
      events.push((ev as CustomEvent<{ pos: number }>).detail.pos),
    );
    e.commands.setTextSelection(find(e, 'one').pos + 2);
    press(e, 'Escape');
    press(e, 'Enter');
    expect(events).toEqual([find(e, 'one').pos]);

    press(e, 'd', { ctrlKey: true }); // jsdom is not a Mac, so Mod = Ctrl
    expect(texts(e)).toEqual(['one', 'one', 'two']);
    e.destroy();
  });
});

describe('code blocks', () => {
  it('read their language from pasted HTML and keep it in the document model', () => {
    const e = new Editor({
      extensions: createExtensions(),
      element: document.createElement('div'),
      content: '<pre><code class="language-rust">let x = 1;</code></pre>',
    });
    expect(blocks(e)[0]).toMatchObject({
      type: 'code',
      text: 'let x = 1;',
      attrs: { language: 'rust' },
    });
    e.destroy();
  });
});
