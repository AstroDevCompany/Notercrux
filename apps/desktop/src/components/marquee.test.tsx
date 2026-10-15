import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, render } from '@testing-library/react';
import { useRef } from 'react';
import { Editor } from '@tiptap/core';
import { createExtensions } from '../editor/extensions';
import { getBlockRange, setBlockRange, rangeOfIndexes, blocksIn } from '../editor/blockRange';
import { BlockRangeOverlay, MarqueeBand, useMarquee } from './MarqueeSelect';

const p = (t: string) => ({ type: 'paragraph', content: [{ type: 'text', text: t }] });

/** jsdom has no layout: give the scroller a size and lay blocks out as 30px rows (20px tall). */
let rectSpy: ReturnType<typeof vi.spyOn>;
beforeEach(() => {
  rectSpy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement,
  ) {
    if (this.classList.contains('content-scroll')) return new DOMRect(0, 0, 800, 600);
    const parent = this.parentElement;
    if (parent?.classList.contains('prose')) {
      const i = [...parent.children].indexOf(this);
      return new DOMRect(100, i * 30, 500, 20);
    }
    return new DOMRect(0, 0, 0, 0);
  });
  document.elementFromPoint = () => null; // ProseMirror asks for it on mouse events; jsdom has none
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
    configurable: true,
    get: () => 800,
  });
});
afterEach(() => {
  cleanup();
  rectSpy.mockRestore();
  delete (HTMLElement.prototype as unknown as { clientWidth?: number }).clientWidth;
});

function setup() {
  const editor = new Editor({
    extensions: createExtensions(),
    element: document.createElement('div'),
    content: { type: 'doc', content: ['a', 'b', 'c', 'd', 'e'].map(p) } as never,
    editorProps: { attributes: { class: 'prose' } },
  });
  function Harness() {
    const art = useRef<HTMLElement>(null);
    const inner = useRef<HTMLDivElement>(null);
    const box = useMarquee(editor, art);
    return (
      <div className="content-scroll">
        <article ref={art} data-testid="page">
          <div data-testid="margin" />
          <div ref={inner} className="page-inner">
            <BlockRangeOverlay editor={editor} inner={inner} />
            <input data-testid="title" />
            <div id="host" />
          </div>
        </article>
        <MarqueeBand box={box} />
      </div>
    );
  }
  const view = render(<Harness />);
  document.querySelector('#host')!.append(editor.view.dom);
  return { editor, view };
}

const down = (el: Element, init: MouseEventInit = {}) => {
  const ev = new MouseEvent('mousedown', {
    bubbles: true,
    cancelable: true,
    button: 0,
    clientX: 40,
    clientY: 0,
    ...init,
  });
  act(() => void el.dispatchEvent(ev));
  return ev;
};
const move = (x: number, y: number) =>
  act(() => void window.dispatchEvent(new MouseEvent('mousemove', { clientX: x, clientY: y })));
const up = () => act(() => void window.dispatchEvent(new MouseEvent('mouseup')));
const names = (e: Editor) =>
  getBlockRange(e.state)
    ? blocksIn(e.state, getBlockRange(e.state)!).map((b) => b.node.textContent)
    : null;

describe('marquee selection from the margins', () => {
  it('a drag that starts in the margin selects the blocks it touches and shows the band', () => {
    const { editor, view } = setup();
    const ev = down(view.getByTestId('margin'), { clientY: 35 });
    expect(ev.defaultPrevented).toBe(true); // no native text selection starts
    move(40, 100);
    expect(names(editor)).toEqual(['b', 'c', 'd']); // rows 30-50, 60-80, 90-110 intersect y 35..100
    expect(document.querySelector('.marquee')).not.toBeNull();
    expect(view.getByRole('status').textContent).toBe('3 blocks selected');
    up();
    expect(document.querySelector('.marquee')).toBeNull();
    expect(names(editor)).toEqual(['b', 'c', 'd']); // the selection stays after the drag
    editor.destroy();
  });

  it('works upward as well and shrinks when the pointer comes back', () => {
    const { editor, view } = setup();
    down(view.getByTestId('margin'), { clientY: 125 });
    move(40, 35);
    expect(names(editor)).toEqual(['b', 'c', 'd', 'e']);
    move(40, 95);
    expect(names(editor)).toEqual(['d', 'e']);
    up();
    editor.destroy();
  });

  it('does not start from text, the title field, or with modifiers/other buttons', () => {
    const { editor, view } = setup();
    const text = editor.view.dom.querySelector('p')!;
    expect(down(text).defaultPrevented).toBe(false);
    expect(down(view.getByTestId('title')).defaultPrevented).toBe(false);
    expect(down(view.getByTestId('margin'), { shiftKey: true }).defaultPrevented).toBe(false);
    expect(down(view.getByTestId('margin'), { button: 2 }).defaultPrevented).toBe(false);
    move(40, 100);
    expect(getBlockRange(editor.state)).toBeNull();
    editor.destroy();
  });

  it('ignores presses on the scrollbar (right of the content box) but not at its edge', () => {
    const { editor, view } = setup();
    expect(down(view.getByTestId('margin'), { clientX: 805 }).defaultPrevented).toBe(false);
    expect(down(view.getByTestId('margin'), { clientX: 790 }).defaultPrevented).toBe(true);
    up();
    editor.destroy();
  });

  it('a plain click in the margin (no drag) just leaves a block selection', () => {
    const { editor, view } = setup();
    setBlockRange(editor, rangeOfIndexes(editor.state, 1, 2)!);
    down(view.getByTestId('margin'), { clientY: 300 });
    up();
    expect(getBlockRange(editor.state)).toBeNull();
    editor.destroy();
  });

  it('tiny movements below the threshold do not start a selection', () => {
    const { editor, view } = setup();
    down(view.getByTestId('margin'), { clientY: 35 });
    move(41, 37);
    expect(getBlockRange(editor.state)).toBeNull();
    expect(document.querySelector('.marquee')).toBeNull();
    up();
    editor.destroy();
  });

  it('the overlay highlights the range, and the text toolbar logic can see it', () => {
    const { editor, view } = setup();
    expect(view.container.querySelector('.block-range')).toBeNull();
    act(() => setBlockRange(editor, rangeOfIndexes(editor.state, 0, 1)!));
    expect(view.container.querySelector('.block-range')).not.toBeNull();
    expect(view.container.querySelector('.page-inner')!.classList.contains('has-block-range')).toBe(
      true,
    );
    act(() => void editor.commands.setTextSelection(2));
    expect(view.container.querySelector('.block-range')).toBeNull();
    expect(view.container.querySelector('.page-inner')!.classList.contains('has-block-range')).toBe(
      false,
    );
    editor.destroy();
  });
});
