import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Icon, type IconName } from './Icon';

export type MenuItem =
  | { type: 'separator' }
  | { type: 'label'; label: string }
  | {
      type?: 'item';
      label: string;
      icon?: IconName;
      danger?: boolean;
      disabled?: boolean;
      /** Marks the current choice (e.g. the active block type). */
      checked?: boolean;
      shortcut?: string;
      onSelect?: () => void;
      /** Opens a submenu instead of running `onSelect`. */
      children?: MenuItem[];
    };

type Action = Extract<MenuItem, { label: string }> & { type?: 'item' };
const isAction = (i: MenuItem): i is Action => i.type === undefined || i.type === 'item';

/**
 * Glass context menu anchored at a point, with separators, submenus and disabled items.
 * Arrow keys move, → opens a submenu, ← closes it, Enter selects, Escape closes everything.
 * Highlighting follows focus (set on pointer move) rather than CSS :hover, which WebKit can fail
 * to repaint inside filtered layers.
 */
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
  const [flip, setFlip] = useState(false);

  useLayoutEffect(() => {
    const el = ref.current!;
    const rect = el.getBoundingClientRect();
    el.style.left = `${Math.max(8, Math.min(x, window.innerWidth - rect.width - 8))}px`;
    el.style.top = `${Math.max(8, Math.min(y, window.innerHeight - rect.height - 8))}px`;
    // Submenus open to the left when there is no room for them on the right.
    setFlip(Math.min(x, window.innerWidth - rect.width - 8) + rect.width + 230 > window.innerWidth);
  }, [x, y, items]);

  useEffect(() => {
    const el = ref.current!;
    const previous = document.activeElement as HTMLElement | null;
    el.querySelector<HTMLElement>('[role=menuitem]:not([aria-disabled=true])')?.focus();
    const away = (e: MouseEvent) => !el.contains(e.target as Node) && onClose();
    window.addEventListener('mousedown', away);
    window.addEventListener('blur', onClose);
    window.addEventListener('resize', onClose);
    return () => {
      window.removeEventListener('mousedown', away);
      window.removeEventListener('blur', onClose);
      window.removeEventListener('resize', onClose);
      previous?.focus?.();
    };
  }, [onClose]);

  return (
    <div
      ref={ref}
      className="menu glass-popover"
      style={{ left: x, top: y }}
      onContextMenu={(e) => e.preventDefault()}
    >
      <MenuList items={items} flip={flip} onClose={onClose} root />
    </div>
  );
}

function MenuList({
  items,
  flip,
  onClose,
  onBack,
  root,
}: {
  items: MenuItem[];
  flip: boolean;
  onClose: () => void;
  onBack?: () => void;
  root?: boolean;
}) {
  const [open, setOpen] = useState<number | null>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const focusables = () => [
    ...(listRef.current?.querySelectorAll<HTMLElement>(
      ':scope > .menu-entry > [role=menuitem]:not([aria-disabled=true])',
    ) ?? []),
  ];

  const onKeyDown = (e: React.KeyboardEvent) => {
    const els = focusables();
    const i = els.indexOf(document.activeElement as HTMLElement);
    switch (e.key) {
      case 'ArrowDown':
        els[(i + 1) % els.length]?.focus();
        break;
      case 'ArrowUp':
        els[(i - 1 + els.length) % els.length]?.focus();
        break;
      case 'Home':
        els[0]?.focus();
        break;
      case 'End':
        els[els.length - 1]?.focus();
        break;
      case 'ArrowRight': {
        const idx = Number((document.activeElement as HTMLElement | null)?.dataset.index);
        const it = items[idx];
        if (it && isAction(it) && it.children) setOpen(idx);
        else return;
        break;
      }
      case 'ArrowLeft':
        if (onBack) onBack();
        else return;
        break;
      case 'Escape':
      case 'Tab':
        onClose();
        break;
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  };

  // Focus the first entry of a freshly opened submenu.
  useEffect(() => {
    if (open === null) return;
    const sub = listRef.current?.querySelector<HTMLElement>(
      `[data-sub="${open}"] [role=menuitem]:not([aria-disabled=true])`,
    );
    sub?.focus();
  }, [open]);

  return (
    <div
      ref={listRef}
      role="menu"
      className={root ? undefined : 'menu-sublist'}
      onKeyDown={onKeyDown}
    >
      {items.map((it, i) => {
        if (it.type === 'separator') return <div key={i} role="separator" className="menu-sep" />;
        if (it.type === 'label') {
          return (
            <div key={i} className="menu-label" role="presentation">
              {it.label}
            </div>
          );
        }
        const hasSub = !!it.children;
        return (
          <div key={i} className="menu-entry">
            <button
              role="menuitem"
              data-index={i}
              aria-disabled={it.disabled || undefined}
              aria-haspopup={hasSub ? 'menu' : undefined}
              aria-expanded={hasSub ? open === i : undefined}
              aria-checked={it.checked}
              className={`menu-item${it.danger ? ' menu-danger' : ''}${it.disabled ? ' is-disabled' : ''}`}
              onMouseMove={(e) => {
                if (it.disabled) return;
                if (e.currentTarget !== document.activeElement) e.currentTarget.focus();
                if (open !== (hasSub ? i : null)) setOpen(hasSub ? i : null);
              }}
              onClick={() => {
                if (it.disabled) return;
                if (hasSub) return setOpen(open === i ? null : i);
                onClose();
                it.onSelect?.();
              }}
            >
              {it.icon ? <Icon name={it.icon} size={15} /> : <span className="menu-icon-gap" />}
              <span className="menu-text">{it.label}</span>
              {it.checked && <Icon name="check" size={14} />}
              {it.shortcut && <kbd className="menu-kbd">{it.shortcut}</kbd>}
              {hasSub && <Icon name="chevron" size={12} />}
            </button>
            {hasSub && open === i && (
              <div className={`menu-sub glass-popover${flip ? ' flip' : ''}`} data-sub={i}>
                <MenuList
                  items={it.children!}
                  flip={flip}
                  onClose={onClose}
                  onBack={() => setOpen(null)}
                />
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
