import { useEffect, useId, useMemo, useRef, useState } from 'react';
import type { Page } from '@notercrux/document-model';
import { api } from '../api';
import { useDialog } from '../hooks/useDialog';
import { actions, breadcrumb, getState, live, pageLabel, useApp } from '../state/app';
import { updateSettings } from '../state/settings';
import { Icon, type IconName } from './Icon';

interface Item {
  id: string;
  group: 'Actions' | 'Pages';
  label: string;
  hint?: string;
  icon?: IconName | string;
  run: () => void;
}

function commands(): Item[] {
  const { currentId } = getState();
  const list: Item[] = [
    {
      id: 'new',
      group: 'Actions',
      label: 'New page',
      icon: 'plus',
      run: () => void actions.createPage(null),
    },
    {
      id: 'settings',
      group: 'Actions',
      label: 'Open settings',
      icon: 'settings',
      run: () => actions.setSettings(true),
    },
    {
      id: 'sidebar',
      group: 'Actions',
      label: 'Toggle sidebar',
      icon: 'sidebar',
      run: () => actions.toggleSidebar(),
    },
    {
      id: 'theme-light',
      group: 'Actions',
      label: 'Theme: Light',
      icon: 'settings',
      run: () => updateSettings({ theme: 'light' }),
    },
    {
      id: 'theme-dark',
      group: 'Actions',
      label: 'Theme: Dark',
      icon: 'settings',
      run: () => updateSettings({ theme: 'dark' }),
    },
    {
      id: 'theme-system',
      group: 'Actions',
      label: 'Theme: Match system',
      icon: 'settings',
      run: () => updateSettings({ theme: 'system' }),
    },
    {
      id: 'glass-reduced',
      group: 'Actions',
      label: 'Reduce transparency',
      icon: 'settings',
      run: () => updateSettings({ transparency: 'reduced' }),
    },
    {
      id: 'glass-on',
      group: 'Actions',
      label: 'Enable glass effects',
      icon: 'settings',
      run: () => updateSettings({ transparency: 'glass' }),
    },
  ];
  if (currentId) {
    list.splice(
      1,
      0,
      {
        id: 'new-child',
        group: 'Actions',
        label: 'New nested page',
        icon: 'plus',
        run: () => void actions.createPage(currentId),
      },
      {
        id: 'rename',
        group: 'Actions',
        label: 'Rename current page',
        icon: 'edit',
        run: () => actions.startRename(currentId),
      },
      {
        id: 'duplicate',
        group: 'Actions',
        label: 'Duplicate current page',
        icon: 'copy',
        run: () => void actions.duplicate(currentId),
      },
      {
        id: 'archive',
        group: 'Actions',
        label: 'Move current page to archive',
        icon: 'archive',
        run: () => void actions.archive(currentId),
      },
    );
  }
  return list;
}

export function CommandPalette() {
  const open = useApp((s) => s.paletteOpen);
  if (!open) return null;
  return <PaletteBody onClose={() => actions.setPalette(false)} />;
}

function PaletteBody({ onClose }: { onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const listId = useId();
  const pages = useApp((s) => s.pages);
  const [query, setQuery] = useState('');
  const [hits, setHits] = useState<Page[]>([]);
  const [active, setActive] = useState(0);
  useDialog(ref, onClose);

  useEffect(() => {
    let stale = false;
    const t = setTimeout(async () => {
      const found = query.trim() ? await api.searchPages(query).catch(() => []) : [];
      if (!stale) setHits(found);
    }, 80);
    return () => {
      stale = true;
      clearTimeout(t);
    };
  }, [query]);

  const items = useMemo<Item[]>(() => {
    const q = query.trim().toLowerCase();
    const cmds = commands().filter((c) => !q || c.label.toLowerCase().includes(q));
    const all = live(pages);
    const byTitle = q
      ? all.filter((p) => pageLabel(p).toLowerCase().includes(q))
      : [...all].sort((a, b) => b.updatedAt - a.updatedAt).slice(0, 8);
    const merged = [...byTitle, ...hits.filter((h) => !byTitle.some((p) => p.id === h.id))];
    const pageItems: Item[] = merged.map((p) => ({
      id: `page:${p.id}`,
      group: 'Pages',
      label: pageLabel(p),
      hint: breadcrumb(pages, p.id).slice(0, -1).map(pageLabel).join(' / ') || undefined,
      icon: p.icon ?? 'page',
      run: () => actions.select(p.id),
    }));
    return q ? [...pageItems, ...cmds] : [...cmds, ...pageItems];
  }, [query, hits, pages]);

  useEffect(() => setActive(0), [query]);
  useEffect(() => {
    document.getElementById(`${listId}-${active}`)?.scrollIntoView({ block: 'nearest' });
  }, [active, listId]);

  const run = (item?: Item) => {
    if (!item) return;
    onClose();
    // Let the dialog restore focus first so actions that move focus (e.g. rename) win.
    setTimeout(item.run, 0);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') setActive((a) => Math.min(a + 1, items.length - 1));
    else if (e.key === 'ArrowUp') setActive((a) => Math.max(a - 1, 0));
    else if (e.key === 'Enter') run(items[active]);
    else return;
    e.preventDefault();
  };

  let lastGroup = '';
  return (
    <div className="scrim scrim-top" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        ref={ref}
        className="palette glass-overlay"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onKeyDown={onKeyDown}
      >
        <div className="palette-input">
          <Icon name="search" size={17} />
          <input
            data-autofocus
            role="combobox"
            aria-expanded="true"
            aria-controls={listId}
            aria-activedescendant={items.length ? `${listId}-${active}` : undefined}
            aria-label="Search pages and commands"
            placeholder="Search pages or run a command…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            spellCheck={false}
          />
        </div>
        <ul id={listId} role="listbox" className="palette-list">
          {items.map((it, i) => {
            const header =
              it.group !== lastGroup
                ? ((lastGroup = it.group),
                  (
                    <li role="presentation" className="palette-group">
                      {it.group}
                    </li>
                  ))
                : null;
            return (
              <li key={it.id} role="presentation" style={{ display: 'contents' }}>
                {header}
                <div
                  id={`${listId}-${i}`}
                  role="option"
                  aria-selected={i === active}
                  className={`palette-item${i === active ? ' is-active' : ''}`}
                  onMouseMove={() => setActive(i)}
                  onClick={() => run(it)}
                >
                  <span className="palette-icon">
                    {it.icon && it.icon.length > 2 ? (
                      <Icon name={it.icon as IconName} size={15} />
                    ) : (
                      it.icon
                    )}
                  </span>
                  <span className="palette-label">{it.label}</span>
                  {it.hint && <span className="palette-hint">{it.hint}</span>}
                </div>
              </li>
            );
          })}
          {!items.length && (
            <li role="presentation" className="palette-empty">
              No results for “{query}”
            </li>
          )}
        </ul>
      </div>
    </div>
  );
}
