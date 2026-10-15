import { useEffect, useRef } from 'react';
import { Icon, type IconName } from './Icon';

export interface MenuItem {
  label: string;
  icon?: IconName;
  danger?: boolean;
  onSelect: () => void;
}

/** Glass context menu anchored at a point. Arrow keys move, Enter selects, Escape closes. */
export function ContextMenu({
  x,
  y,
  items,
  onClose,
}: {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current!;
    const previous = document.activeElement as HTMLElement | null;
    el.querySelector<HTMLElement>('[role=menuitem]')?.focus();
    const rect = el.getBoundingClientRect();
    el.style.left = `${Math.max(8, Math.min(x, window.innerWidth - rect.width - 8))}px`;
    el.style.top = `${Math.max(8, Math.min(y, window.innerHeight - rect.height - 8))}px`;
    const away = (e: MouseEvent) => !el.contains(e.target as Node) && onClose();
    window.addEventListener('mousedown', away);
    window.addEventListener('blur', onClose);
    return () => {
      window.removeEventListener('mousedown', away);
      window.removeEventListener('blur', onClose);
      previous?.focus?.();
    };
  }, [x, y, onClose]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    const els = [...ref.current!.querySelectorAll<HTMLElement>('[role=menuitem]')];
    const i = els.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'ArrowDown') els[(i + 1) % els.length]?.focus();
    else if (e.key === 'ArrowUp') els[(i - 1 + els.length) % els.length]?.focus();
    else if (e.key === 'Home') els[0]?.focus();
    else if (e.key === 'End') els[els.length - 1]?.focus();
    else if (e.key === 'Escape' || e.key === 'Tab') onClose();
    else return;
    e.preventDefault();
    e.stopPropagation();
  };

  return (
    <div
      ref={ref}
      className="menu glass-popover"
      role="menu"
      style={{ left: x, top: y }}
      onKeyDown={onKeyDown}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((it) => (
        <button
          key={it.label}
          role="menuitem"
          className={`menu-item${it.danger ? ' menu-danger' : ''}`}
          // Highlight follows the pointer through focus rather than CSS :hover, which WebKit can
          // fail to repaint inside composited, filtered layers. One item is highlighted at a time.
          onMouseMove={(e) => {
            if (e.currentTarget !== document.activeElement) e.currentTarget.focus();
          }}
          onClick={() => {
            onClose();
            it.onSelect();
          }}
        >
          {it.icon && <Icon name={it.icon} size={15} />}
          {it.label}
        </button>
      ))}
    </div>
  );
}
