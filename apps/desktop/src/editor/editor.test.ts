import { describe, expect, it } from 'vitest';
import { Editor } from '@tiptap/core';
import { fromProseMirror } from '@notercrux/document-model';
import { extensions } from './extensions';
import { filterSlashItems, SLASH_ITEMS } from './slashMenu';

const p = (text: string) => ({ type: 'paragraph', content: [{ type: 'text', text }] });
const make = (content: unknown[]) =>
  new Editor({
    extensions,
    content: { type: 'doc', content } as never,
    element: document.createElement('div'),
  });
// The editor keeps an empty trailing paragraph after a final non-paragraph block; ignore it here.
const texts = (e: Editor) =>
  fromProseMirror(e.getJSON() as never)
    .blocks.map((b) => b.text)
    .filter(Boolean);

/** Sends a real keydown through the view, as a keypress would (unlike commands.keyboardShortcut). */
const press = (e: Editor, key: string, mods: Partial<KeyboardEventInit> = {}) =>
  e.view.dom.dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...mods }),
  );

describe('block movement', () => {
  it('moves the block under the cursor up and down, keeping the cursor in it', () => {
    const e = make([p('one'), p('two'), p('three')]);
    e.commands.setTextSelection(7); // inside "two"
    press(e, 'ArrowUp', { altKey: true });
    expect(texts(e)).toEqual(['two', 'one', 'three']);
    press(e, 'ArrowDown', { altKey: true });
    press(e, 'ArrowDown', { altKey: true });
    expect(texts(e)).toEqual(['one', 'three', 'two']);
    expect(e.state.selection.$from.parent.textContent).toBe('two');
    press(e, 'ArrowDown', { altKey: true }); // already last: no-op
    expect(texts(e)).toEqual(['one', 'three', 'two']);
    e.destroy();
  });

  it('moves list items among their siblings, children included', () => {
    const li = (t: string, kids: unknown[] = []) => ({
      type: 'listItem',
      content: [p(t), ...kids],
    });
    const e = make([
      {
        type: 'bulletList',
        content: [li('a'), li('b', [{ type: 'bulletList', content: [li('b1')] }])],
      },
    ]);
    let at = 0;
    e.state.doc.descendants((n, pos) => {
      if (n.isText && n.text === 'b') at = pos + 1;
    });
    e.commands.setTextSelection(at);
    press(e, 'ArrowUp', { ctrlKey: true, shiftKey: true }); // jsdom is not a Mac, so Mod = Ctrl
    const doc = fromProseMirror(e.getJSON() as never);
    expect(doc.blocks.map((b) => b.text).filter(Boolean)).toEqual(['b', 'a']);
    expect(doc.blocks[0]!.children![0]!.text).toBe('b1');
    e.destroy();
  });
});

describe('blocks', () => {
  it('assigns unique ids to blocks, including pasted duplicates', () => {
    const e = make([
      { type: 'paragraph', attrs: { blockId: 'same' }, content: [{ type: 'text', text: 'x' }] },
      { type: 'paragraph', attrs: { blockId: 'same' }, content: [{ type: 'text', text: 'y' }] },
    ]);
    e.commands.insertContentAt(1, 'z'); // any transaction triggers id repair
    const ids = fromProseMirror(e.getJSON() as never).blocks.map((b) => b.id);
    expect(new Set(ids).size).toBe(2);
    e.destroy();
  });

  it('wraps a paragraph in a callout that round-trips to the document model', () => {
    const e = make([p('careful')]);
    e.commands.setTextSelection(2);
    e.commands.toggleCallout();
    const [block] = fromProseMirror(e.getJSON() as never).blocks;
    expect(block).toMatchObject({ type: 'callout', text: 'careful', attrs: { icon: '💡' } });
    e.commands.toggleCallout();
    expect(texts(e)).toEqual(['careful']);
    e.destroy();
  });
});

describe('slash menu filtering', () => {
  it('lists everything for an empty query and ranks title prefixes first', () => {
    expect(filterSlashItems('')).toHaveLength(SLASH_ITEMS.length);
    expect(filterSlashItems('head').map((i) => i.id)).toEqual(['h1', 'h2', 'h3']);
    expect(filterSlashItems('todo')[0]!.id).toBe('todo');
    expect(filterSlashItems('checkbox')[0]!.id).toBe('todo'); // keyword match
    expect(filterSlashItems('qqqq')).toEqual([]);
  });
});
