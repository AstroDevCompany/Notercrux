import { useEffect, useState, type RefObject } from 'react';
import type { Editor } from '@tiptap/core';
import {
  blocksIn,
  clearBlockRange,
  getBlockRange,
  pickBlocks,
  setBlockRange,
} from '../editor/blockRange';

interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Things inside the page that handle their own mouse input: a margin drag must not start on them. */
const OWN_INPUT =
  '.prose, .title-row, .record-props, .backlinks, .block-handle, .save-state, button, input, select, textarea, a, label';
const DRAG_THRESHOLD = 4;
const EDGE = 48;

/**
 * Rubber-band selection from the page margins. Press in the empty space beside (or below) the text
 * and drag: every top-level block the band touches becomes selected. The band scrolls the page when
 * the pointer nears the top or bottom edge. Returns the band rectangle (client coordinates) to draw.
 */
export function useMarquee(
  editor: Editor | null,
  article: RefObject<HTMLElement | null>,
): Box | null {
  const [box, setBox] = useState<Box | null>(null);

  useEffect(() => {
    const art = article.current;
    if (!editor || !art) return;
    const scroller = art.closest<HTMLElement>('.content-scroll');
    let drag: {
      x0: number;
      docY0: number;
      x: number;
      y: number;
      moved: boolean;
      raf: number;
    } | null = null;

    const scrollTop = () => scroller?.scrollTop ?? 0;
    const blockRect = (pos: number) => {
      const dom = editor.view.nodeDOM(pos);
      return dom instanceof HTMLElement ? dom.getBoundingClientRect() : null;
    };

    const apply = () => {
      if (!drag?.moved) return;
      // The start is anchored in page coordinates so it stays put while the page scrolls.
      const y0 = drag.docY0 - scrollTop();
      const top = Math.min(y0, drag.y);
      const bottom = Math.max(y0, drag.y);
      setBox({
        left: Math.min(drag.x0, drag.x),
        top,
        width: Math.abs(drag.x - drag.x0),
        height: bottom - top,
      });
      const range = pickBlocks(editor.state, top, bottom, blockRect);
      if (range) setBlockRange(editor, range);
      else clearBlockRange(editor, 'start');
    };

    const tick = () => {
      if (!drag) return;
      if (scroller && drag.moved) {
        const r = scroller.getBoundingClientRect();
        let dy = 0;
        if (drag.y < r.top + EDGE) dy = -Math.ceil((r.top + EDGE - drag.y) / 4);
        else if (drag.y > r.bottom - EDGE) dy = Math.ceil((drag.y - (r.bottom - EDGE)) / 4);
        if (dy) {
          scroller.scrollTop += dy;
          apply();
        }
      }
      drag.raf = requestAnimationFrame(tick);
    };

    const onMove = (e: MouseEvent) => {
      if (!drag) return;
      drag.x = e.clientX;
      drag.y = e.clientY;
      if (!drag.moved) {
        if (Math.hypot(e.clientX - drag.x0, e.clientY + scrollTop() - drag.docY0) < DRAG_THRESHOLD)
          return;
        drag.moved = true;
        document.documentElement.classList.add('is-marquee');
      }
      apply();
    };

    const end = () => {
      if (!drag) return;
      const { moved } = drag;
      cancelAnimationFrame(drag.raf);
      drag = null;
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', end);
      window.removeEventListener('blur', end);
      document.documentElement.classList.remove('is-marquee');
      setBox(null);
      // A plain click in the margin just leaves any block selection.
      if (!moved) clearBlockRange(editor, 'start');
      else editor.view.focus();
    };

    const onDown = (e: MouseEvent) => {
      if (e.button !== 0 || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
      const target = e.target as HTMLElement;
      if (target.closest(OWN_INPUT)) return;
      // Ignore presses on the scrollbar itself.
      if (scroller) {
        const r = scroller.getBoundingClientRect();
        if (e.clientX > r.left + scroller.clientWidth) return;
      }
      e.preventDefault(); // no native text selection, and the editor keeps focus
      drag = {
        x0: e.clientX,
        docY0: e.clientY + scrollTop(),
        x: e.clientX,
        y: e.clientY,
        moved: false,
        raf: 0,
      };
      window.addEventListener('mousemove', onMove);
      window.addEventListener('mouseup', end);
      window.addEventListener('blur', end);
      drag.raf = requestAnimationFrame(tick);
    };

    art.addEventListener('mousedown', onDown);
    return () => {
      art.removeEventListener('mousedown', onDown);
      end();
    };
  }, [editor, article]);

  return box;
}

/** The rubber band while dragging. */
export function MarqueeBand({ box }: { box: Box | null }) {
  if (!box) return null;
  return <div className="marquee" aria-hidden="true" style={box} />;
}

/** Highlight over the selected blocks, plus a polite announcement for screen readers. */
export function BlockRangeOverlay({
  editor,
  inner,
}: {
  editor: Editor | null;
  inner: RefObject<HTMLElement | null>;
}) {
  const [state, setState] = useState<{ box: Box; count: number } | null>(null);

  useEffect(() => {
    if (!editor) return;
    const update = () => {
      const host = inner.current;
      const range = getBlockRange(editor.state);
      host?.classList.toggle('has-block-range', !!range);
      if (!host || !range) return setState(null);
      const blocks = blocksIn(editor.state, range);
      const first = editor.view.nodeDOM(blocks[0]!.pos);
      const last = editor.view.nodeDOM(blocks[blocks.length - 1]!.pos);
      if (!(first instanceof HTMLElement) || !(last instanceof HTMLElement)) return setState(null);
      const h = host.getBoundingClientRect();
      const a = first.getBoundingClientRect();
      const b = last.getBoundingClientRect();
      const prose = editor.view.dom.getBoundingClientRect();
      setState({
        count: blocks.length,
        box: {
          left: prose.left - h.left - 10,
          top: a.top - h.top - 4,
          width: prose.width + 20,
          height: b.bottom - a.top + 8,
        },
      });
    };
    update();
    editor.on('transaction', update);
    window.addEventListener('resize', update);
    return () => {
      editor.off('transaction', update);
      window.removeEventListener('resize', update);
    };
  }, [editor, inner]);

  return (
    <>
      {state && <div className="block-range" aria-hidden="true" style={state.box} />}
      <div className="sr-only" role="status" aria-live="polite">
        {state ? `${state.count} block${state.count === 1 ? '' : 's'} selected` : ''}
      </div>
    </>
  );
}
