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
  anchor: RefObject<HTMLElement | null>;
  onClose: () => void;
  children: ReactNode;
  label: string;
  width?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number; maxHeight: number } | null>(null);
  useDialog(ref, onClose);

  useLayoutEffect(() => {
    const r = anchor.current?.getBoundingClientRect();
    if (!r) return;
    const left = Math.max(8, Math.min(r.left, window.innerWidth - width - 8));
    const top = r.bottom + 6;
    setPos({ left, top, maxHeight: Math.max(160, window.innerHeight - top - 12) });
  }, [anchor, width]);

  useEffect(() => {
    const away = (e: MouseEvent) => {
      const el = ref.current;
      if (el && !el.contains(e.target as Node) && !anchor.current?.contains(e.target as Node))
        onClose();
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
