import { describe, expect, it } from 'vitest';
import { Editor } from '@tiptap/core';
import { fromProseMirror } from '@notercrux/document-model';
import {
  blocksIn,
  clearBlockRange,
  deleteBlockRange,
  duplicateBlockRange,
  getBlockRange,
  pickBlocks,
  rangeOfIndexes,
  setBlockRange,
  turnRangeInto,
} from './blockRange';
import { createExtensions } from './extensions';

const p = (text: string) => ({ type: 'paragraph', content: [{ type: 'text', text }] });
const h = (text: string) => ({
  type: 'heading',
  attrs: { level: 2 },
  content: [{ type: 'text', text }],
});
const list = (...items: string[]) => ({
  type: 'bulletList',
  content: items.map((t) => ({ type: 'listItem', content: [p(t)] })),
});
const make = (content: unknown[]) =>
  new Editor({
    extensions: createExtensions(),
    content: { type: 'doc', content } as never,
    element: document.createElement('div'),
  });
const texts = (e: Editor) =>
  fromProseMirror(e.getJSON() as never)
    .blocks.flatMap((b) => [b.text, ...(b.children ?? []).map((c) => c.text)])
    .filter(Boolean);
const types = (e: Editor) => e.getJSON().content!.map((n) => n.type);
const press = (e: Editor, key: string, init: KeyboardEventInit = {}) =>
  e.view.dom.dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }),
  );

/** A fake DataTransfer good enough for copy/cut/paste handlers. */
const clipboard = () => {
  const data = new Map<string, string>();
  const dt = {
    clearData: () => data.clear(),
    setData: (t: string, v: string) => void data.set(t, v),
    getData: (t: string) => data.get(t) ?? '',
  };
  return { dt, data };
};
const fire = (e: Editor, type: 'copy' | 'cut' | 'paste', dt: unknown) => {
  const ev = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(ev, 'clipboardData', { value: dt });
  e.view.dom.dispatchEvent(ev);
  return ev;
};

describe('block range model', () => {
  it('maps block indexes to positions and back', () => {
    const e = make([p('a'), h('b'), p('c'), p('d')]);
    const r = rangeOfIndexes(e.state, 1, 2)!;
    expect(blocksIn(e.state, r).map((b) => b.node.textContent)).toEqual(['b', 'c']);
    expect(rangeOfIndexes(e.state, 2, 1)).toEqual(r); // order of arguments does not matter
    expect(rangeOfIndexes(e.state, -5, 99)).toEqual({ from: 0, to: e.state.doc.content.size });
    e.destroy();
  });

  it('picks the blocks a vertical span touches, ignoring horizontal position', () => {
    const e = make([p('a'), p('b'), p('c'), p('d')]);
    // Blocks laid out as 30px rows: a 0-20, b 30-50, c 60-80, d 90-110.
    const tops = new Map<number, number>();
    let i = 0;
    e.state.doc.forEach((_n, off) => tops.set(off, 30 * i++));
    const rectOf = (pos: number) => ({ top: tops.get(pos)!, bottom: tops.get(pos)! + 20 });
    const pick = (a: number, b: number) =>
      blocksIn(e.state, pickBlocks(e.state, a, b, rectOf) ?? { from: 0, to: 0 }).map(
        (x) => x.node.textContent,
      );
    expect(pick(35, 65)).toEqual(['b', 'c']); // touches the bottom of b and the top of c
    expect(pick(21, 29)).toEqual([]); // the gap between blocks selects nothing
    expect(pick(-100, 5)).toEqual(['a']);
    expect(pick(0, 500)).toEqual(['a', 'b', 'c', 'd']);
    expect(pickBlocks(e.state, 21, 29, rectOf)).toBeNull();
    e.destroy();
  });

  it('treats a whole list as one block', () => {
    const e = make([p('a'), list('x', 'y', 'z'), p('b')]);
    const r = rangeOfIndexes(e.state, 1, 1)!;
    expect(blocksIn(e.state, r)).toHaveLength(1);
    expect(blocksIn(e.state, r)[0]!.node.type.name).toBe('bulletList');
    e.destroy();
  });
});

describe('block range state', () => {
  it('is set with a matching selection and cleared by selection or document changes', () => {
    const e = make([p('a'), p('b'), p('c')]);
    const r = rangeOfIndexes(e.state, 0, 1)!;
    setBlockRange(e, r);
    expect(getBlockRange(e.state)).toEqual(r);
    expect(e.state.selection.from).toBeGreaterThanOrEqual(r.from);
    expect(e.state.selection.to).toBeLessThanOrEqual(r.to);

    e.commands.setTextSelection(2);
    expect(getBlockRange(e.state)).toBeNull();

    setBlockRange(e, r);
    e.commands.insertContentAt(1, 'x'); // a document change
    expect(getBlockRange(e.state)).toBeNull();
    e.destroy();
  });

  it('survives the attribute-only transaction that repairs duplicate ids', () => {
    const e = make([p('a'), p('b')]);
    setBlockRange(e, rangeOfIndexes(e.state, 0, 0)!);
    duplicateBlockRange(e);
    const r = getBlockRange(e.state)!;
    expect(r).not.toBeNull();
    expect(blocksIn(e.state, r).map((b) => b.node.textContent)).toEqual(['a']); // the copy is selected
    const ids = fromProseMirror(e.getJSON() as never).blocks.map((b) => b.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(texts(e)).toEqual(['a', 'a', 'b']);
    e.destroy();
  });
});

describe('actions on a block range', () => {
  it('Backspace and Delete remove whole blocks, leaving no empty block behind', () => {
    for (const key of ['Backspace', 'Delete']) {
      const e = make([h('title'), p('a'), p('b'), p('keep')]);
      setBlockRange(e, rangeOfIndexes(e.state, 0, 2)!);
      press(e, key);
      expect(texts(e)).toEqual(['keep']);
      expect(types(e)).toEqual(['paragraph']);
      expect(getBlockRange(e.state)).toBeNull();
      e.destroy();
    }
  });

  it('deleting every block leaves one empty paragraph', () => {
    const e = make([p('a'), p('b')]);
    setBlockRange(e, rangeOfIndexes(e.state, 0, 1)!);
    expect(deleteBlockRange(e)).toBe(true);
    expect(e.state.doc.childCount).toBe(1);
    expect(e.state.doc.child(0).type.name).toBe('paragraph');
    expect(e.state.doc.textContent).toBe('');
    e.destroy();
  });

  it('⌘/Ctrl+D duplicates the whole range', () => {
    const e = make([p('a'), p('b'), p('c')]);
    setBlockRange(e, rangeOfIndexes(e.state, 0, 1)!);
    press(e, 'd', { ctrlKey: true }); // jsdom is not a Mac, so Mod = Ctrl
    expect(texts(e)).toEqual(['a', 'b', 'a', 'b', 'c']);
    e.destroy();
  });

  it('Esc/arrows/Enter leave the range without changing the document; Alt+arrows are ignored', () => {
    const doc = () => [h('title'), p('a'), p('b')];
    const e = make(doc());
    const r = rangeOfIndexes(e.state, 0, 1)!;
    setBlockRange(e, r); // the first transaction assigns block ids
    clearBlockRange(e);
    const before = JSON.stringify(e.getJSON());

    setBlockRange(e, r);
    press(e, 'ArrowUp', { altKey: true });
    press(e, 'ArrowDown', { altKey: true });
    expect(getBlockRange(e.state)).toEqual(r); // swallowed: nothing moved, range intact
    expect(JSON.stringify(e.getJSON())).toBe(before);

    press(e, 'Escape');
    expect(getBlockRange(e.state)).toBeNull();
    expect(e.state.selection.$from.parent.textContent).toBe('title'); // cursor at the start

    setBlockRange(e, r);
    press(e, 'Enter');
    expect(e.state.selection.$from.parent.textContent).toBe('a'); // cursor at the end
    expect(JSON.stringify(e.getJSON())).toBe(before);

    setBlockRange(e, r);
    press(e, 'ArrowDown');
    expect(getBlockRange(e.state)).toBeNull();
    e.destroy();
  });

  it('typing a character leaves the range instead of replacing it', () => {
    const e = make([p('a'), p('b')]);
    setBlockRange(e, rangeOfIndexes(e.state, 0, 1)!);
    press(e, 'x');
    expect(getBlockRange(e.state)).toBeNull();
    expect(texts(e)).toEqual(['a', 'b']); // nothing was deleted
    e.destroy();
  });

  it('turns every selected block into another kind', () => {
    const e = make([p('a'), p('b'), p('c')]);
    setBlockRange(e, rangeOfIndexes(e.state, 0, 1)!);
    turnRangeInto(e, 'quote');
    expect(types(e).slice(0, 3)).toEqual(['blockquote', 'blockquote', 'paragraph']);
    expect(texts(e)).toEqual(['a', 'b', 'c']);
    e.destroy();
  });

  it('clearBlockRange returns false when there is no range', () => {
    const e = make([p('a')]);
    expect(clearBlockRange(e)).toBe(false);
    e.destroy();
  });
});

describe('clipboard with a block range', () => {
  it('copy writes whole blocks (types included) and leaves the document alone', () => {
    const e = make([h('Title'), p('body'), p('other')]);
    setBlockRange(e, rangeOfIndexes(e.state, 0, 1)!);
    const { dt, data } = clipboard();
    const ev = fire(e, 'copy', dt);
    expect(ev.defaultPrevented).toBe(true);
    expect(data.get('text/html')).toMatch(/<h2[^>]*>Title<\/h2>/);
    expect(data.get('text/html')).toMatch(/<p[^>]*>body<\/p>/);
    expect(data.get('text/html')).not.toContain('other');
    expect(data.get('text/plain')).toContain('Title');
    expect(texts(e)).toEqual(['Title', 'body', 'other']);
    e.destroy();
  });

  it('cut copies then removes the blocks', () => {
    const e = make([p('a'), p('b'), p('c')]);
    setBlockRange(e, rangeOfIndexes(e.state, 0, 1)!);
    const { dt, data } = clipboard();
    fire(e, 'cut', dt);
    expect(data.get('text/plain')).toContain('a');
    expect(texts(e)).toEqual(['c']);
    e.destroy();
  });

  it('paste replaces the range with a fresh paragraph for the pasted content to land in', () => {
    const e = make([h('Title'), p('a'), p('keep')]);
    setBlockRange(e, rangeOfIndexes(e.state, 0, 1)!);
    fire(e, 'paste', { getData: () => '', files: [], types: [] });
    expect(getBlockRange(e.state)).toBeNull();
    expect(types(e)).toEqual(['paragraph', 'paragraph']);
    expect(texts(e)).toEqual(['keep']);
    expect(e.state.selection.$from.parent.type.name).toBe('paragraph'); // cursor sits in the new empty block
    e.destroy();
  });

  it('without a range, copy is left to the editor', () => {
    const e = make([p('a')]);
    const { dt, data } = clipboard();
    const ev = fire(e, 'copy', dt);
    expect(ev.defaultPrevented).toBe(false);
    expect(data.size).toBe(0);
    e.destroy();
  });
});
