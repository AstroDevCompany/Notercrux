import { useState } from 'react';
import { actions, pageLabel, useApp } from '../state/app';
import { Icon } from './Icon';

/** Archived pages (top-most archived ancestors only) with restore / delete-forever. */
export function ArchiveList() {
  const pages = useApp((s) => s.pages);
  const [open, setOpen] = useState(false);
  const byId = new Map(pages.map((p) => [p.id, p]));
  const roots = pages.filter(
    (p) => p.archivedAt && !(p.parentId && byId.get(p.parentId)?.archivedAt),
  );
  if (!roots.length) return null;

  return (
    <section className="archive" aria-label="Archive">
      <button className="archive-toggle" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name="chevron" size={12} className={open ? 'rot90' : ''} />
        <Icon name="archive" size={14} />
        Archive <span className="badge">{roots.length}</span>
      </button>
      {open && (
        <ul className="archive-list">
          {roots.map((p) => (
            <li key={p.id} className="archive-row">
              <span className="tree-title">{pageLabel(p)}</span>
              <button
                className="icon-btn"
                aria-label={`Restore ${pageLabel(p)}`}
                onClick={() => void actions.restore(p.id)}
              >
                <Icon name="restore" size={14} />
              </button>
              <button
                className="icon-btn"
                aria-label={`Delete ${pageLabel(p)} forever`}
                onClick={() =>
                  actions.ask({
                    title: 'Delete forever?',
                    message: `“${pageLabel(p)}” and everything inside it will be permanently deleted. This cannot be undone.`,
                    confirmLabel: 'Delete forever',
                    destructive: true,
                    onConfirm: () => void actions.deletePermanently(p.id),
                  })
                }
              >
                <Icon name="trash" size={14} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
