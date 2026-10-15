import { setLinkMenuIndex, useLinkMenu } from '../editor/pageLink';
import { Icon } from './Icon';

/** Glass popover for the `[[` page picker. Keyboard handling lives in the Suggestion plugin. */
export function PageLinkMenu() {
  const s = useLinkMenu();
  if (!s.open || !s.rect) return null;
  const below = window.innerHeight - s.rect.bottom > 300;
  const left = Math.min(s.rect.left, window.innerWidth - 300);
  const style = below
    ? { left, top: s.rect.bottom + 6 }
    : { left, bottom: window.innerHeight - s.rect.top + 6 };
  return (
    <div className="slash glass-popover" style={style} role="listbox" aria-label="Link to page">
      {s.items.length === 0 && <div className="slash-empty">No pages</div>}
      {s.items.map((item, i) => (
        <div
          key={item.id}
          role="option"
          aria-selected={i === s.index}
          className={`slash-item link-item${i === s.index ? ' is-active' : ''}`}
          onMouseMove={() => i !== s.index && setLinkMenuIndex(i)}
          onMouseDown={(e) => {
            e.preventDefault();
            s.select(item);
          }}
        >
          <span className="link-icon">
            {item.create ? (
              <Icon name="plus" size={14} />
            ) : (
              (item.icon ?? <Icon name={item.kind === 'database' ? 'table' : 'page'} size={14} />)
            )}
          </span>
          <span className="slash-title">{item.create ? `Create “${item.title}”` : item.title}</span>
        </div>
      ))}
    </div>
  );
}
