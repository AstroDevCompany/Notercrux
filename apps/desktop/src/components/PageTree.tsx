import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type KeyboardEvent,
} from 'react';
import type { Page } from '@notercrux/document-model';
import { actions, live, pageLabel, useApp } from '../state/app';
import { ContextMenu, type MenuItem } from './ContextMenu';
import { Icon } from './Icon';

const ROW_H = 30;
const OVERSCAN = 10;

interface Row {
  page: Page;
  depth: number;
  hasChildren: boolean;
  expanded: boolean;
  setSize: number;
  pos: number;
}

type DropState = '' | 'dragging' | 'before' | 'after' | 'inside';

/** Flattens the visible part of the tree in one pass (children are indexed once, not re-scanned). */
export function flatten(pages: Page[], expandedMap: Record<string, boolean>): Row[] {
  const byParent = new Map<string | null, Page[]>();
  for (const p of pages) {
    const list = byParent.get(p.parentId);
    if (list) list.push(p);
    else byParent.set(p.parentId, [p]);
  }
  for (const list of byParent.values()) list.sort((a, b) => a.position - b.position);

  const rows: Row[] = [];
  const walk = (parentId: string | null, depth: number) => {
    const kids = byParent.get(parentId) ?? [];
    kids.forEach((page, i) => {
      // Records of a database are managed in its table, not in the sidebar tree.
      const hasChildren = page.kind !== 'database' && (byParent.get(page.id)?.length ?? 0) > 0;
      const expanded = hasChildren && !!expandedMap[page.id];
      rows.push({ page, depth, hasChildren, expanded, setSize: kids.length, pos: i + 1 });
      if (expanded) walk(page.id, depth + 1);
    });
  };
  walk(null, 1);
  return rows;
}

interface Handlers {
  select(id: string): void;
  focus(id: string): void;
  keyDown(e: KeyboardEvent, id: string): void;
  contextMenu(e: React.MouseEvent, id: string): void;
  menuButton(e: React.MouseEvent, id: string): void;
  dragStart(e: DragEvent, id: string): void;
  dragOver(e: DragEvent, id: string): void;
  drop(e: DragEvent, id: string): void;
  dragEnd(): void;
  renameDone(id: string, value: string): void;
  renameCancel(id: string): void;
}

interface RowProps {
  row: Row;
  top: number;
  selected: boolean;
  tabStop: boolean;
  renaming: boolean;
  drop: DropState;
  h: Handlers;
}

const TreeRow = memo(function TreeRow({
  row,
  top,
  selected,
  tabStop,
  renaming,
  drop,
  h,
}: RowProps) {
  const p = row.page;
  return (
    <li
      role="treeitem"
      aria-level={row.depth}
      aria-setsize={row.setSize}
      aria-posinset={row.pos}
      aria-selected={selected}
      aria-expanded={row.hasChildren ? row.expanded : undefined}
      data-row-id={p.id}
      tabIndex={tabStop ? 0 : -1}
      draggable={!renaming}
      className={`tree-row${selected ? ' is-selected' : ''}${drop === 'dragging' ? ' is-dragging' : ''}${
        drop && drop !== 'dragging' ? ` drop-${drop}` : ''
      }`}
      style={{ paddingLeft: 6 + (row.depth - 1) * 14, top, height: ROW_H }}
      onDragStart={(e) => h.dragStart(e, p.id)}
      onDragOver={(e) => h.dragOver(e, p.id)}
      onDrop={(e) => h.drop(e, p.id)}
      onDragEnd={h.dragEnd}
      onClick={() => h.select(p.id)}
      onFocus={(e) => e.target === e.currentTarget && h.focus(p.id)}
      onKeyDown={(e) => h.keyDown(e, p.id)}
      onContextMenu={(e) => h.contextMenu(e, p.id)}
    >
      <button
        tabIndex={-1}
        aria-hidden="true"
        className={`tree-chevron${row.hasChildren ? '' : ' is-hidden'}${row.expanded ? ' is-open' : ''}`}
        onClick={(e) => {
          e.stopPropagation();
          actions.toggleExpanded(p.id);
        }}
      >
        <Icon name="chevron" size={12} />
      </button>
      <span className="tree-icon">
        {p.icon ?? <Icon name={p.kind === 'database' ? 'table' : 'page'} size={15} />}
      </span>
      {renaming ? (
        <input
          className="tree-rename"
          defaultValue={p.title}
          autoFocus
          aria-label="Page title"
          onClick={(e) => e.stopPropagation()}
          onFocus={(e) => e.currentTarget.select()}
          onBlur={(e) => h.renameDone(p.id, e.currentTarget.value)}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === 'Enter') e.currentTarget.blur();
            if (e.key === 'Escape') h.renameCancel(p.id);
          }}
        />
      ) : (
        <span className={`tree-title${p.title.trim() ? '' : ' is-untitled'}`}>{pageLabel(p)}</span>
      )}
      <span className="tree-actions">
        <button
          tabIndex={-1}
          className="icon-btn"
          aria-label={`Add page inside ${pageLabel(p)}`}
          onClick={(e) => {
            e.stopPropagation();
            void actions.createPage(p.id);
          }}
        >
          <Icon name="plus" size={14} />
        </button>
        <button
          tabIndex={-1}
          className="icon-btn"
          aria-label={`More actions for ${pageLabel(p)}`}
          onClick={(e) => h.menuButton(e, p.id)}
        >
          <Icon name="more" size={14} />
        </button>
      </span>
    </li>
  );
});

export function PageTree() {
  const pages = useApp((s) => s.pages);
  const expandedMap = useApp((s) => s.expanded);
  const currentId = useApp((s) => s.currentId);
  const renamingId = useApp((s) => s.renamingId);
  const livePages = useMemo(() => live(pages), [pages]);
  const rows = useMemo(() => flatten(livePages, expandedMap), [livePages, expandedMap]);
  const [focusId, setFocusId] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; page: Page } | null>(null);
  const [drag, setDrag] = useState<{
    id: string;
    over: string | null;
    where: 'before' | 'after' | 'inside';
  } | null>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const [range, setRange] = useState({ start: 0, end: 60 });

  const indexOf = useMemo(() => new Map(rows.map((r, i) => [r.page.id, i])), [rows]);
  const current = pages.find((p) => p.id === currentId);
  const tabStopId =
    (focusId && indexOf.has(focusId) ? focusId : null) ??
    (currentId && indexOf.has(currentId) ? currentId : null) ??
    rows[0]?.page.id ??
    null;

  // ---- windowing: render only the rows in (or near) view ------------------------------------
  const scroller = () => listRef.current?.closest<HTMLElement>('.sidebar-scroll') ?? null;
  const updateRange = useCallback(() => {
    const el = scroller();
    if (!el) return;
    const top = Math.max(0, el.scrollTop - (listRef.current?.offsetTop ?? 0));
    const start = Math.max(0, Math.floor(top / ROW_H) - OVERSCAN);
    const end = Math.min(rows.length, Math.ceil((top + el.clientHeight) / ROW_H) + OVERSCAN);
    setRange((r) => (r.start === start && r.end === end ? r : { start, end }));
  }, [rows.length]);

  useLayoutEffect(() => {
    updateRange();
    const el = scroller();
    if (!el) return;
    el.addEventListener('scroll', updateRange, { passive: true });
    const ro = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(updateRange);
    ro?.observe(el);
    return () => {
      el.removeEventListener('scroll', updateRange);
      ro?.disconnect();
    };
  }, [updateRange]);

  /** Scrolls the row into view (it may not be rendered yet) and focuses it once it is. */
  const focusRow = useCallback(
    (id: string) => {
      setFocusId(id);
      const i = indexOf.get(id);
      const el = scroller();
      if (i !== undefined && el) {
        const rowTop = i * ROW_H + (listRef.current?.offsetTop ?? 0);
        if (rowTop < el.scrollTop) el.scrollTop = rowTop;
        else if (rowTop + ROW_H > el.scrollTop + el.clientHeight)
          el.scrollTop = rowTop + ROW_H - el.clientHeight;
      }
      requestAnimationFrame(() =>
        listRef.current?.querySelector<HTMLElement>(`[data-row-id="${id}"]`)?.focus(),
      );
    },
    [indexOf],
  );

  // Keep the selected page in view when it changes from elsewhere (palette, links, history).
  useEffect(() => {
    const i = currentId ? indexOf.get(currentId) : undefined;
    const el = scroller();
    if (i === undefined || !el) return;
    const rowTop = i * ROW_H + (listRef.current?.offsetTop ?? 0);
    if (rowTop < el.scrollTop || rowTop + ROW_H > el.scrollTop + el.clientHeight) {
      el.scrollTop = Math.max(0, rowTop - el.clientHeight / 3);
    }
  }, [currentId, indexOf]);

  // ---- handlers (stable identity; always read the latest state through the ref) -------------
  const latest = useRef({ rows, indexOf, pages, drag, focusRow, currentId });
  latest.current = { rows, indexOf, pages, drag, focusRow, currentId };

  const handlers = useMemo<Handlers>(() => {
    const descendantIds = (id: string): Set<string> => {
      const out = new Set([id]);
      for (let grew = true; grew;) {
        grew = false;
        for (const p of latest.current.pages) {
          if (p.parentId && out.has(p.parentId) && !out.has(p.id)) {
            out.add(p.id);
            grew = true;
          }
        }
      }
      return out;
    };
    const rowOf = (id: string) => {
      const { rows: rs, indexOf: ix } = latest.current;
      const i = ix.get(id);
      return i === undefined ? undefined : { row: rs[i]!, i, rows: rs };
    };
    const anchorPos = (e: { currentTarget: Element }, dx: number, dy = 0) => {
      const r = e.currentTarget.getBoundingClientRect();
      return { x: r.left + dx, y: r.bottom + dy };
    };
    return {
      select: (id) => {
        actions.select(id);
        setFocusId(id);
      },
      focus: (id) => setFocusId(id),
      keyDown: (e, id) => {
        if ((e.target as HTMLElement).tagName === 'INPUT') return;
        const found = rowOf(id);
        if (!found) return;
        const { row, i, rows: rs } = found;
        const p = row.page;
        const { focusRow: focus } = latest.current;
        if (e.altKey && e.key.startsWith('Arrow')) {
          const how = {
            ArrowUp: 'up',
            ArrowDown: 'down',
            ArrowRight: 'indent',
            ArrowLeft: 'outdent',
          }[e.key] as 'up' | 'down' | 'indent' | 'outdent' | undefined;
          if (how) {
            e.preventDefault();
            void actions.shift(p.id, how).then(() => focus(p.id));
          }
          return;
        }
        switch (e.key) {
          case 'ArrowDown':
            if (rs[i + 1]) focus(rs[i + 1]!.page.id);
            break;
          case 'ArrowUp':
            if (rs[i - 1]) focus(rs[i - 1]!.page.id);
            break;
          case 'Home':
            focus(rs[0]!.page.id);
            break;
          case 'End':
            focus(rs[rs.length - 1]!.page.id);
            break;
          case 'PageDown':
            focus(rs[Math.min(rs.length - 1, i + 10)]!.page.id);
            break;
          case 'PageUp':
            focus(rs[Math.max(0, i - 10)]!.page.id);
            break;
          case 'ArrowRight':
            if (row.hasChildren && !row.expanded) actions.toggleExpanded(p.id, true);
            else if (row.expanded) focus(rs[i + 1]!.page.id);
            break;
          case 'ArrowLeft':
            if (row.expanded) actions.toggleExpanded(p.id, false);
            else if (p.parentId) focus(p.parentId);
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
            void actions.archive(p.id);
            break;
          case 'ContextMenu': {
            const a = anchorPos(e, 24);
            setMenu({ ...a, page: p });
            break;
          }
          case 'F10':
            if (!e.shiftKey) return;
            setMenu({ ...anchorPos(e, 24), page: p });
            break;
          default:
            return;
        }
        e.preventDefault();
      },
      contextMenu: (e, id) => {
        e.preventDefault();
        const f = rowOf(id);
        if (f) setMenu({ x: e.clientX, y: e.clientY, page: f.row.page });
      },
      menuButton: (e, id) => {
        e.stopPropagation();
        const f = rowOf(id);
        if (f) setMenu({ ...anchorPos(e, 0, 4), page: f.row.page });
      },
      dragStart: (e, id) => {
        e.dataTransfer.effectAllowed = 'move';
        e.dataTransfer.setData('text/plain', id);
        setDrag({ id, over: null, where: 'inside' });
      },
      dragOver: (e, id) => {
        const d = latest.current.drag;
        if (!d || descendantIds(d.id).has(id)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = 'move';
        const r = e.currentTarget.getBoundingClientRect();
        const y = (e.clientY - r.top) / r.height;
        const where = y < 0.28 ? 'before' : y > 0.72 ? 'after' : 'inside';
        if (d.over !== id || d.where !== where) setDrag({ ...d, over: id, where });
      },
      drop: (e, id) => {
        const d = latest.current.drag;
        if (!d) return;
        e.preventDefault();
        setDrag(null);
        if (!descendantIds(d.id).has(id)) void actions.place(d.id, id, d.where);
      },
      dragEnd: () => setDrag(null),
      renameDone: (id, value) => void actions.rename(id, value),
      renameCancel: (id) => {
        actions.cancelRename();
        latest.current.focusRow(id);
      },
    };
  }, []);

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

  // Window of rows to render, plus the tab-stop row (so keyboard users can always Tab into the tree).
  const shown: number[] = [];
  for (let i = range.start; i < Math.min(range.end, rows.length); i++) shown.push(i);
  const tabIndex = tabStopId ? indexOf.get(tabStopId) : undefined;
  if (tabIndex !== undefined && (tabIndex < range.start || tabIndex >= range.end))
    shown.push(tabIndex);

  const menuItems = (p: Page): MenuItem[] => [
    { label: 'Add nested page', icon: 'plus', onSelect: () => void actions.createPage(p.id) },
    { label: 'Rename', icon: 'edit', onSelect: () => actions.startRename(p.id) },
    { label: 'Duplicate', icon: 'copy', onSelect: () => void actions.duplicate(p.id) },
    {
      label: 'Move to archive',
      icon: 'archive',
      danger: true,
      onSelect: () => void actions.archive(p.id),
    },
  ];

  return (
    <>
      <ul
        ref={listRef}
        className="tree"
        role="tree"
        aria-label="Pages"
        aria-rowcount={rows.length}
        style={{ height: rows.length * ROW_H }}
      >
        {shown.map((i) => {
          const row = rows[i]!;
          const p = row.page;
          // A database stays highlighted while one of its (sidebar-hidden) records is open.
          const selected =
            p.id === currentId || (p.kind === 'database' && current?.parentId === p.id);
          let drop: DropState = '';
          if (drag?.id === p.id) drop = 'dragging';
          else if (drag?.over === p.id) drop = drag.where;
          return (
            <TreeRow
              key={p.id}
              row={row}
              top={i * ROW_H}
              selected={selected}
              tabStop={p.id === tabStopId}
              renaming={renamingId === p.id}
              drop={drop}
              h={handlers}
            />
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
