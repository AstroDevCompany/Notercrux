import { useEffect, useRef } from 'react';
import { useSlashState } from '../editor/slashMenu';

/** Glass popover listing block types. Keyboard handling lives in the Suggestion plugin. */
export function SlashMenu() {
  const s = useSlashState();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    ref.current?.querySelector('[aria-selected=true]')?.scrollIntoView({ block: 'nearest' });
  }, [s.index, s.open]);

  if (!s.open || !s.rect) return null;
  const below = window.innerHeight - s.rect.bottom > 320;
  const style = below
    ? { left: Math.min(s.rect.left, window.innerWidth - 280), top: s.rect.bottom + 6 }
    : {
        left: Math.min(s.rect.left, window.innerWidth - 280),
        bottom: window.innerHeight - s.rect.top + 6,
      };

  return (
    <div
      ref={ref}
      className="slash glass-popover"
      style={style}
      role="listbox"
      aria-label="Insert block"
    >
      {s.items.length === 0 && <div className="slash-empty">No matching blocks</div>}
      {s.items.map((item, i) => (
        <div
          key={item.id}
          role="option"
          aria-selected={i === s.index}
          className={`slash-item${i === s.index ? ' is-active' : ''}`}
          // mousedown (not click) keeps the editor selection intact.
          onMouseDown={(e) => {
            e.preventDefault();
            s.select(item);
          }}
        >
          <span className="slash-title">{item.title}</span>
          <span className="slash-hint">{item.hint}</span>
        </div>
      ))}
    </div>
  );
}
