import { useEffect, useRef, useState } from 'react';
import type { Editor } from '@tiptap/core';
import { NodeSelection } from '@tiptap/pm/state';
import { blockAt, selectBlock } from '../editor/blockOps';
import { getBlockRange } from '../editor/blockRange';
import type { Anchor } from './database/Popover';

/**
 * Handle shown beside the hovered block.
 * - Click: selects the block and opens its options menu.
 * - Drag: moves the block (selects it as a node and hands ProseMirror a "move" drag, so its own
 *   drop handling and drop cursor do the rest).
 * Keyboard users select a block with Esc, move it with Alt+↑/↓ and open its menu with Enter.
 */
export function BlockHandle({
  editor,
  container,
  onOpenMenu,
  frozen,
}: {
  editor: Editor | null;
  container: React.RefObject<HTMLElement | null>;
  onOpenMenu: (pos: number, anchor: Anchor) => void;
  /** True while a menu is open: the handle stays on its block instead of following the pointer. */
  frozen: boolean;
}) {
  const [top, setTop] = useState<number | null>(null);
  const target = useRef<{ pos: number; dom: HTMLElement } | null>(null);
  const el = useRef<HTMLButtonElement>(null);
  const frozenRef = useRef(frozen);
  frozenRef.current = frozen;

  useEffect(() => {
    const host = container.current;
    if (!editor || !host) return;
    const onMove = (e: MouseEvent) => {
      if (frozenRef.current || getBlockRange(editor.state)) return;
      const view = editor.view;
      const rect = view.dom.getBoundingClientRect();
      // Probe just inside the editor's left edge so hovering the handle gutter still resolves a block.
      const hit = view.posAtCoords({ left: Math.max(e.clientX, rect.left + 8), top: e.clientY });
      if (!hit) return;
      const ref = blockAt(editor, hit.pos);
      if (!ref) return;
      const dom = view.nodeDOM(ref.pos);
      if (!(dom instanceof HTMLElement)) return;
      target.current = { pos: ref.pos, dom };
      const hostRect = host.getBoundingClientRect();
      setTop(dom.getBoundingClientRect().top - hostRect.top + host.scrollTop + 2);
    };
    const onLeave = (e: MouseEvent) => {
      if (frozenRef.current) return;
      if (!(e.relatedTarget instanceof Node) || !host.contains(e.relatedTarget)) setTop(null);
    };
    // A block selection replaces the handle until the pointer moves onto a block again.
    const onTx = () => {
      if (getBlockRange(editor.state)) setTop(null);
    };
    editor.on('transaction', onTx);
    host.addEventListener('mousemove', onMove);
    host.addEventListener('mouseleave', onLeave);
    return () => {
      editor.off('transaction', onTx);
      host.removeEventListener('mousemove', onMove);
      host.removeEventListener('mouseleave', onLeave);
    };
  }, [editor, container]);

  // When the menu closes, hide the handle until the pointer moves again.
  useEffect(() => {
    if (!frozen) setTop(null);
  }, [frozen]);

  if (!editor || top === null) return null;

  return (
    <button
      ref={el}
      className="block-handle"
      style={{ top }}
      draggable
      aria-label="Block options (click) or move (drag)"
      aria-haspopup="menu"
      title="Click for options · drag to move"
      onClick={() => {
        const t = target.current;
        if (!t || !el.current) return;
        selectBlock(editor, t.pos);
        onOpenMenu(t.pos, el.current);
      }}
      onDragStart={(e) => {
        const t = target.current;
        if (!t) return;
        const view = editor.view;
        const sel = NodeSelection.create(view.state.doc, t.pos);
        view.dispatch(view.state.tr.setSelection(sel));
        view.dragging = { slice: sel.content(), move: true };
        e.dataTransfer.effectAllowed = 'move';
        e.dataTransfer.setData('text/plain', ' ');
        e.dataTransfer.setDragImage(t.dom, 0, 0);
      }}
      onDragEnd={() => {
        editor.view.dragging = null;
        setTop(null);
      }}
    >
      ⋮⋮
    </button>
  );
}
