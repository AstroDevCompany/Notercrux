import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from 'react';
import { createPortal } from 'react-dom';
import { useDialog } from '../../hooks/useDialog';

/** Anything with a bounding box: an element ref, or a plain object for a point/virtual anchor. */
export interface Anchor {
  getBoundingClientRect(): DOMRect;
  contains?(node: Node): boolean;
}
const rectOf = (a: RefObject<HTMLElement | null> | Anchor) =>
  'current' in a ? a.current?.getBoundingClientRect() : a.getBoundingClientRect();
const holds = (a: RefObject<HTMLElement | null> | Anchor, n: Node) =>
  'current' in a ? !!a.current?.contains(n) : !!a.contains?.(n);

/**
 * Glass popover anchored under an element. Rendered in a portal so it is never clipped by (or
 * mispositioned inside) scrolling or transformed ancestors such as the virtualized table.
 */
export function Popover({
  anchor,
  onClose,
  children,
  label,
  width = 340,
}: {
  anchor: RefObject<HTMLElement | null> | Anchor;
  onClose: () => void;
  children: ReactNode;
  label: string;
  width?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number; maxHeight: number } | null>(null);
  useDialog(ref, onClose);

  useLayoutEffect(() => {
    const r = rectOf(anchor);
    const el = ref.current;
    if (!r || !el) return;
    const pad = 8;
    const natural = el.scrollHeight; // measured before any max-height is applied
    const left = Math.max(pad, Math.min(r.left, window.innerWidth - width - pad));
    let top = r.bottom + 6;
    if (top + natural > window.innerHeight - pad) {
      // Not enough room below: prefer opening above the anchor, else pin to the window and scroll.
      const above = r.top - 6 - natural;
      top = above >= pad ? above : Math.max(pad, window.innerHeight - pad - natural);
    }
    setPos({ left, top, maxHeight: window.innerHeight - top - pad });
  }, [anchor, width]);

  useEffect(() => {
    const away = (e: MouseEvent) => {
      const el = ref.current;
      if (el && !el.contains(e.target as Node) && !holds(anchor, e.target as Node)) onClose();
    };
    window.addEventListener('mousedown', away);
    return () => window.removeEventListener('mousedown', away);
  }, [anchor, onClose]);

  return createPortal(
    <div
      ref={ref}
      className="popover glass-popover"
      role="dialog"
      aria-label={label}
      tabIndex={-1}
      style={{ ...(pos ?? { left: -9999, top: 0 }), width, maxHeight: pos?.maxHeight }}
    >
      {children}
    </div>,
    document.body,
  );
}
