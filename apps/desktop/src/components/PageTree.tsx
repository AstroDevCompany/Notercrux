import { useMemo, useRef, useState, type KeyboardEvent } from 'react';
import type { Page } from '@notercrux/document-model';
import { actions, childrenOf, live, pageLabel, useApp } from '../state/app';
import { ContextMenu, type MenuItem } from './ContextMenu';
import { Icon } from './Icon';

interface Row {
  page: Page;
  depth: number;
  hasChildren: boolean;
  expanded: boolean;
  setSize: number;
  pos: number;
}

function flatten(pages: Page[], expandedMap: Record<string, boolean>): Row[] {
  const rows: Row[] = [];
  const walk = (parentId: string | null, depth: number) => {
    const kids = childrenOf(pages, parentId);
    kids.forEach((page, i) => {
      const hasChildren = childrenOf(pages, page.id).length > 0;
      const expanded = hasChildren && !!expandedMap[page.id];
      rows.push({ page, depth, hasChildren, expanded, setSize: kids.length, pos: i + 1 });
      if (expanded) walk(page.id, depth + 1);
    });
  };
  walk(null, 1);
  return rows;
}

export function PageTree() {
  const pages = useApp((s) => s.pages);
  const expandedMap = useApp((s) => s.expanded);
  const currentId = useApp((s) => s.currentId);
  const renamingId = useApp((s) => s.renamingId);
  const livePages = useMemo(() => live(pages), [pages]);
  const rows = useMemo(() => flatten(livePages, expandedMap), [livePages, expandedMap]);
  const [focusId, setFocusId] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; page: Page } | null>(null);
  const listRef = useRef<HTMLUListElement>(null);

  const tabStop =
    rows.find((r) => r.page.id === (focusId ?? currentId))?.page.id ?? rows[0]?.page.id;

  const focusRow = (id: string) => {
    setFocusId(id);
    requestAnimationFrame(() =>
      listRef.current?.querySelector<HTMLElement>(`[data-row-id="${id}"]`)?.focus(),
    );
  };

  const menuItems = (p: Page): MenuItem[] => [
    { label: 'Add nested page', icon: 'plus', onSelect: () => void actions.createPage(p.id) },
    { label: 'Rename', icon: 'edit', onSelect: () => actions.startRename(p.id) },
    { label: 'Duplicate', icon: 'copy', onSelect: () => void actions.duplicate(p.id) },
    { label: 'Move to archive', icon: 'archive', danger: true, onSelect: () => archive(p) },
  ];

  const archive = (p: Page) => void actions.archive(p.id);

  const onKeyDown = (e: KeyboardEvent, row: Row, i: number) => {
    if ((e.target as HTMLElement).tagName === 'INPUT') return;
    const p = row.page;
    switch (e.key) {
      case 'ArrowDown':
        if (rows[i + 1]) focusRow(rows[i + 1]!.page.id);
        break;
      case 'ArrowUp':
        if (rows[i - 1]) focusRow(rows[i - 1]!.page.id);
        break;
      case 'Home':
        focusRow(rows[0]!.page.id);
        break;
      case 'End':
        focusRow(rows[rows.length - 1]!.page.id);
        break;
      case 'ArrowRight':
        if (row.hasChildren && !row.expanded) actions.toggleExpanded(p.id, true);
        else if (row.expanded) focusRow(rows[i + 1]!.page.id);
        break;
      case 'ArrowLeft':
        if (row.expanded) actions.toggleExpanded(p.id, false);
        else if (p.parentId) focusRow(p.parentId);
        break;
      case 'Enter':
      case ' ':
        actions.select(p.id);
        break;
      case 'F2':
        actions.startRename(p.id);
        break;
      case 'Delete':
      case 'Backspace':
        archive(p);
        break;
      case 'ContextMenu': {
        const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
        setMenu({ x: r.left + 24, y: r.bottom, page: p });
        break;
      }
      case 'F10':
        if (!e.shiftKey) return;
        {
          const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
          setMenu({ x: r.left + 24, y: r.bottom, page: p });
        }
        break;
      default:
        return;
    }
    e.preventDefault();
  };

  if (!rows.length) {
    return (
      <div className="tree-empty">
        <p>No pages yet.</p>
        <button className="btn btn-primary" onClick={() => void actions.createPage(null)}>
          Create your first page
        </button>
      </div>
    );
  }

  return (
    <>
      <ul ref={listRef} className="tree" role="tree" aria-label="Pages">
        {rows.map((row, i) => {
          const p = row.page;
          const selected = p.id === currentId;
          return (
            <li
              key={p.id}
              role="treeitem"
              aria-level={row.depth}
              aria-setsize={row.setSize}
              aria-posinset={row.pos}
              aria-selected={selected}
              aria-expanded={row.hasChildren ? row.expanded : undefined}
              data-row-id={p.id}
              tabIndex={p.id === tabStop ? 0 : -1}
              className={`tree-row${selected ? ' is-selected' : ''}`}
              style={{ paddingLeft: 6 + (row.depth - 1) * 14 }}
              onClick={() => (actions.select(p.id), setFocusId(p.id))}
              onFocus={(e) => e.target === e.currentTarget && setFocusId(p.id)}
              onKeyDown={(e) => onKeyDown(e, row, i)}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ x: e.clientX, y: e.clientY, page: p });
              }}
            >
              <button
                tabIndex={-1}
                aria-hidden="true"
                className={`tree-chevron${row.hasChildren ? '' : ' is-hidden'}${row.expanded ? ' is-open' : ''}`}
                onClick={(e) => (e.stopPropagation(), actions.toggleExpanded(p.id))}
              >
                <Icon name="chevron" size={12} />
              </button>
              <span className="tree-icon">{p.icon ?? <Icon name="page" size={15} />}</span>
              {renamingId === p.id ? (
                <input
                  className="tree-rename"
                  defaultValue={p.title}
                  autoFocus
                  aria-label="Page title"
                  onClick={(e) => e.stopPropagation()}
                  onFocus={(e) => e.currentTarget.select()}
                  onBlur={(e) => void actions.rename(p.id, e.currentTarget.value)}
                  onKeyDown={(e) => {
                    e.stopPropagation();
                    if (e.key === 'Enter') e.currentTarget.blur();
                    if (e.key === 'Escape') {
                      actions.cancelRename();
                      focusRow(p.id);
                    }
                  }}
                />
              ) : (
                <span className={`tree-title${p.title.trim() ? '' : ' is-untitled'}`}>
                  {pageLabel(p)}
                </span>
              )}
              <span className="tree-actions">
                <button
                  tabIndex={-1}
                  className="icon-btn"
                  aria-label={`Add page inside ${pageLabel(p)}`}
                  onClick={(e) => (e.stopPropagation(), void actions.createPage(p.id))}
                >
                  <Icon name="plus" size={14} />
                </button>
                <button
                  tabIndex={-1}
                  className="icon-btn"
                  aria-label={`More actions for ${pageLabel(p)}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    const r = e.currentTarget.getBoundingClientRect();
                    setMenu({ x: r.left, y: r.bottom + 4, page: p });
                  }}
                >
                  <Icon name="more" size={14} />
                </button>
              </span>
            </li>
          );
        })}
      </ul>
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={menuItems(menu.page)}
          onClose={() => setMenu(null)}
        />
      )}
    </>
  );
}
