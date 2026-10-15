import { useEffect, useRef, useState } from 'react';
import type { Editor } from '@tiptap/core';
import { NodeSelection } from '@tiptap/pm/state';

/**
 * Drag handle shown beside the hovered top-level block. Dragging selects the block as a node and
 * hands ProseMirror a "move" drag, so its own drop handling (and the drop cursor) does the rest.
 * Keyboard users move blocks with Alt/Cmd+Shift+Arrow Up/Down instead.
 */
export function BlockHandle({
  editor,
  container,
}: {
  editor: Editor | null;
  container: React.RefObject<HTMLElement | null>;
}) {
  const [top, setTop] = useState<number | null>(null);
  const target = useRef<{ pos: number; dom: HTMLElement } | null>(null);

  useEffect(() => {
    const host = container.current;
    if (!editor || !host) return;
    const onMove = (e: MouseEvent) => {
      const view = editor.view;
      const rect = view.dom.getBoundingClientRect();
      // Probe just inside the editor's left edge so hovering the handle gutter still resolves a block.
      const hit = view.posAtCoords({ left: Math.max(e.clientX, rect.left + 8), top: e.clientY });
      if (!hit) return;
      const $pos = view.state.doc.resolve(hit.pos);
      if ($pos.depth < 1) return;
      const pos = $pos.before(1);
      const dom = view.nodeDOM(pos) as HTMLElement | null;
      if (!dom || !(dom instanceof HTMLElement)) return;
      target.current = { pos, dom };
      const hostRect = host.getBoundingClientRect();
      setTop(dom.getBoundingClientRect().top - hostRect.top + host.scrollTop + 2);
    };
    const onLeave = (e: MouseEvent) => {
      if (!(e.relatedTarget instanceof Node) || !host.contains(e.relatedTarget)) setTop(null);
    };
    host.addEventListener('mousemove', onMove);
    host.addEventListener('mouseleave', onLeave);
    return () => {
      host.removeEventListener('mousemove', onMove);
      host.removeEventListener('mouseleave', onLeave);
    };
  }, [editor, container]);

  if (!editor || top === null) return null;

  return (
    <div
      className="block-handle"
      style={{ top }}
      draggable
      role="presentation"
      title="Drag to move block"
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
    </div>
  );
}
