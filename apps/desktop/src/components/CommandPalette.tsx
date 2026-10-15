import { Fragment, useEffect, useId, useMemo, useRef, useState, type ReactNode } from 'react';
import type { Page } from '@notercrux/document-model';
import { api, type SearchHit } from '../api';
import { useDialog } from '../hooks/useDialog';
import { actions, breadcrumb, canGo, getState, live, pageLabel, useApp } from '../state/app';
import { updateSettings } from '../state/settings';
import { bindingOf, formatBinding, type CommandId } from '../state/shortcuts';
import { Icon, type IconName } from './Icon';

type Group = 'Recent' | 'Pages' | 'Found in content' | 'Actions' | 'Create';

interface Item {
  id: string;
  group: Group;
  label: string;
  hint?: ReactNode;
  icon?: IconName | string;
  shortcut?: string;
  run: () => void;
}

const MARK_ON = '\u0001';
const MARK_OFF = '\u0002';

/** Renders a search excerpt, highlighting the spans the backend marked. Never trusts markup. */
export function Highlighted({ text }: { text: string }) {
  const parts: ReactNode[] = [];
  let rest = text;
  let key = 0;
  while (rest) {
    const on = rest.indexOf(MARK_ON);
    if (on < 0) {
      parts.push(rest);
      break;
    }
    const off = rest.indexOf(MARK_OFF, on);
    if (off < 0) {
      parts.push(rest.replace(MARK_ON, ''));
      break;
    }
    if (on > 0) parts.push(rest.slice(0, on));
    parts.push(<mark key={key++}>{rest.slice(on + 1, off)}</mark>);
    rest = rest.slice(off + 1);
  }
  return (
    <>
      {parts.map((p, i) => (
        <Fragment key={i}>{p}</Fragment>
      ))}
    </>
  );
}

const bindingHint = (id: CommandId) => formatBinding(bindingOf(id));

function commands(): Item[] {
  const s = getState();
  const { currentId } = s;
  const current = s.pages.find((p) => p.id === currentId);
  const act = (
    id: string,
    label: string,
    icon: IconName,
    run: () => void,
    shortcut?: string,
  ): Item => ({
    id,
    group: 'Actions',
    label,
    icon,
    run,
    shortcut,
  });
  const list: Item[] = [
    act('new', 'New page', 'plus', () => void actions.createPage(null), bindingHint('newPage')),
    act('new-db', 'New database', 'table', () => void actions.createDatabase(null)),
  ];
  if (canGo(s, -1))
    list.push(act('back', 'Go back', 'back', () => actions.go(-1), bindingHint('back')));
  if (canGo(s, 1))
    list.push(act('forward', 'Go forward', 'forward', () => actions.go(1), bindingHint('forward')));
  if (currentId && current) {
    if (current.kind === 'database') {
      list.push(
        act(
          'new-record',
          'New record in this database',
          'plus',
          () => void actions.createPage(currentId),
        ),
      );
    } else {
      list.push(
        act('new-child', 'New nested page', 'plus', () => void actions.createPage(currentId)),
      );
    }
    list.push(
      act('rename', 'Rename current page', 'edit', () => actions.startRename(currentId)),
      act('duplicate', 'Duplicate current page', 'copy', () => void actions.duplicate(currentId)),
    );
    if (current.kind !== 'database') {
      list.push(
        act(
          'export-md',
          'Export page as Markdown…',
          'copy',
          () => void actions.exportPage(currentId, 'md'),
        ),
        act(
          'export-json',
          'Export page as JSON (lossless)…',
          'copy',
          () => void actions.exportPage(currentId, 'json'),
        ),
      );
    }
    list.push(
      act(
        'archive',
        'Move current page to archive',
        'archive',
        () => void actions.archive(currentId),
      ),
    );
  }
  list.push(
    act('import', 'Import Markdown or JSON file…', 'page', () => void actions.importFile()),
    act(
      'sidebar',
      'Toggle sidebar',
      'sidebar',
      () => actions.toggleSidebar(),
      bindingHint('toggleSidebar'),
    ),
    act(
      'settings',
      'Open settings',
      'settings',
      () => actions.setSettings(true),
      bindingHint('settings'),
    ),
    act('theme-light', 'Theme: Light', 'settings', () => updateSettings({ theme: 'light' })),
    act('theme-dark', 'Theme: Dark', 'settings', () => updateSettings({ theme: 'dark' })),
    act('theme-system', 'Theme: Match system', 'settings', () =>
      updateSettings({ theme: 'system' }),
    ),
    act('glass-reduced', 'Reduce transparency', 'settings', () =>
      updateSettings({ transparency: 'reduced' }),
    ),
    act('glass-on', 'Enable glass effects', 'settings', () =>
      updateSettings({ transparency: 'glass' }),
    ),
  );
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
  const history = useApp((s) => s.history);
  const [query, setQuery] = useState('');
  const [hits, setHits] = useState<SearchHit[]>([]);
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
    const all = live(pages);
    const pageItem = (p: Page, group: Group, hint?: ReactNode): Item => ({
      id: `${group}:${p.id}`,
      group,
      label: pageLabel(p),
      hint: hint ?? (breadcrumb(pages, p.id).slice(0, -1).map(pageLabel).join(' / ') || undefined),
      icon: p.icon ?? (p.kind === 'database' ? 'table' : 'page'),
      run: () => actions.open(p.id),
    });
    const cmds = commands().filter((c) => !q || c.label.toLowerCase().includes(q));

    if (!q) {
      // Recent: most recently visited first, then most recently edited.
      const seen = new Set<string>();
      const recent: Page[] = [];
      for (const id of [...history.stack].reverse()) {
        const p = all.find((x) => x.id === id);
        if (p && !seen.has(id) && id !== getState().currentId) {
          seen.add(id);
          recent.push(p);
        }
      }
      for (const p of [...all].sort((a, b) => b.updatedAt - a.updatedAt)) {
        if (!seen.has(p.id) && p.id !== getState().currentId) {
          seen.add(p.id);
          recent.push(p);
        }
      }
      return [...cmds, ...recent.slice(0, 8).map((p) => pageItem(p, 'Recent'))];
    }

    const byTitle = all.filter((p) => pageLabel(p).toLowerCase().includes(q));
    const titleIds = new Set(byTitle.map((p) => p.id));
    const content = hits.filter((h) => !titleIds.has(h.page.id));
    const out: Item[] = [
      ...byTitle.slice(0, 8).map((p) => pageItem(p, 'Pages')),
      ...content.map((h) => pageItem(h.page, 'Found in content', <Highlighted text={h.snippet} />)),
      ...cmds,
    ];
    const title = query.trim();
    out.push({
      id: 'create-page',
      group: 'Create',
      label: `Create page “${title}”`,
      icon: 'plus',
      run: () => void actions.createPage(null, { title }),
    });
    return out;
  }, [query, hits, pages, history]);

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
    else if (e.key === 'Home' && e.altKey) setActive(0);
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
            placeholder="Search pages, content, or run a command…"
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
                  onMouseMove={() => active !== i && setActive(i)}
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
                  {it.shortcut && <kbd>{it.shortcut}</kbd>}
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
