import { useEffect, useMemo, useRef, useState } from 'react';
import {
  applyView,
  newKey,
  type DbRecord,
  type Page,
  type Property,
  type View,
} from '@notercrux/document-model';
import { actions, pageLabel } from '../../state/app';
import { ContextMenu } from '../ContextMenu';
import { Icon } from '../Icon';
import { Cell } from './Cells';
import { FilterEditor, PropertiesEditor, SortEditor } from './Controls';
import { Popover } from './Popover';
import { useDatabase } from './useDatabase';

const ROW_H = 38;
const OVERSCAN = 8;
const lastViewKey = (db: string) => `notercrux.view.${db}`;

type Panel = 'filter' | 'sort' | 'props' | null;

export function DatabaseView({ page }: { page: Page }) {
  const { schema, records, error, saveSchema, setValue, updateView } = useDatabase(page.id);
  const [viewId, setViewId] = useState<string | null>(() => {
    try {
      return localStorage.getItem(lastViewKey(page.id));
    } catch {
      return null;
    }
  });
  const [panel, setPanel] = useState<Panel>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; view: View } | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const filterBtn = useRef<HTMLButtonElement>(null);
  const sortBtn = useRef<HTMLButtonElement>(null);
  const propsBtn = useRef<HTMLButtonElement>(null);

  const view = useMemo(
    () => schema?.views.find((v) => v.id === viewId) ?? schema?.views[0] ?? null,
    [schema, viewId],
  );
  const rows = useMemo(
    () => (schema && view ? applyView(records, schema, view) : []),
    [records, schema, view],
  );

  const selectView = (id: string) => {
    setViewId(id);
    try {
      localStorage.setItem(lastViewKey(page.id), id);
    } catch {
      /* ignore */
    }
  };

  if (error) {
    return (
      <div className="page-state" role="alert">
        <h2>This database can’t be opened</h2>
        <p>{error}</p>
      </div>
    );
  }
  if (!schema || !view) return <div className="page-state" aria-busy="true" />;

  const visible = schema.properties.filter((p) => !view.hidden.includes(p.id));
  const columns = `minmax(240px, 2.2fr) ${visible.map(() => 'minmax(150px, 1fr)').join(' ')}`;
  const addView = () => {
    const v: View = {
      id: newKey('v'),
      name: `View ${schema.views.length + 1}`,
      sorts: [],
      filters: [],
      filterMode: 'and',
      hidden: [],
    };
    void saveSchema({ ...schema, views: [...schema.views, v] });
    selectView(v.id);
    setRenaming(v.id);
  };
  const removeView = (v: View) => {
    if (schema.views.length < 2) return;
    void saveSchema({ ...schema, views: schema.views.filter((x) => x.id !== v.id) });
    if (view.id === v.id) selectView(schema.views.find((x) => x.id !== v.id)!.id);
  };
  const filtered = view.filters.length > 0;

  return (
    <article className="page db-page" aria-label={pageLabel(page)}>
      <div className="db-head">
        <DbTitle page={page} />
        <div className="db-toolbar" role="toolbar" aria-label="Database controls">
          <div className="db-views" role="tablist" aria-label="Views">
            {schema.views.map((v) =>
              renaming === v.id ? (
                <input
                  key={v.id}
                  className="view-rename"
                  autoFocus
                  aria-label="View name"
                  defaultValue={v.name}
                  onFocus={(e) => e.currentTarget.select()}
                  onBlur={(e) => {
                    setRenaming(null);
                    const n = e.currentTarget.value.trim();
                    if (n && n !== v.name)
                      void saveSchema({
                        ...schema,
                        views: schema.views.map((x) => (x.id === v.id ? { ...x, name: n } : x)),
                      });
                  }}
                  onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
                />
              ) : (
                <button
                  key={v.id}
                  role="tab"
                  aria-selected={v.id === view.id}
                  className={`view-tab${v.id === view.id ? ' is-on' : ''}`}
                  onClick={() => selectView(v.id)}
                  onDoubleClick={() => setRenaming(v.id)}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setMenu({ x: e.clientX, y: e.clientY, view: v });
                  }}
                >
                  {v.name}
                </button>
              ),
            )}
            <button className="icon-btn" aria-label="Add view" title="Add view" onClick={addView}>
              <Icon name="plus" size={14} />
            </button>
          </div>
          <span className="db-spacer" />
          <button
            ref={filterBtn}
            className={`btn btn-tool${filtered ? ' is-active' : ''}`}
            aria-haspopup="dialog"
            aria-expanded={panel === 'filter'}
            onClick={() => setPanel(panel === 'filter' ? null : 'filter')}
          >
            <Icon name="filter" size={14} /> Filter{filtered ? ` · ${view.filters.length}` : ''}
          </button>
          <button
            ref={sortBtn}
            className={`btn btn-tool${view.sorts.length ? ' is-active' : ''}`}
            aria-haspopup="dialog"
            aria-expanded={panel === 'sort'}
            onClick={() => setPanel(panel === 'sort' ? null : 'sort')}
          >
            <Icon name="sort" size={14} /> Sort{view.sorts.length ? ` · ${view.sorts.length}` : ''}
          </button>
          <button
            ref={propsBtn}
            className="btn btn-tool"
            aria-haspopup="dialog"
            aria-expanded={panel === 'props'}
            onClick={() => setPanel(panel === 'props' ? null : 'props')}
          >
            Properties
          </button>
          <button
            className="btn btn-primary"
            onClick={() => void actions.createPage(page.id, { select: false })}
          >
            New
          </button>
        </div>
      </div>

      {panel === 'filter' && (
        <Popover anchor={filterBtn} label="Filters" width={520} onClose={() => setPanel(null)}>
          <FilterEditor
            schema={schema}
            view={view}
            update={(patch) => void updateView(view.id, patch)}
          />
        </Popover>
      )}
      {panel === 'sort' && (
        <Popover anchor={sortBtn} label="Sorting" width={460} onClose={() => setPanel(null)}>
          <SortEditor
            schema={schema}
            view={view}
            update={(patch) => void updateView(view.id, patch)}
          />
        </Popover>
      )}
      {panel === 'props' && (
        <Popover anchor={propsBtn} label="Properties" width={440} onClose={() => setPanel(null)}>
          <PropertiesEditor
            dbId={page.id}
            schema={schema}
            view={view}
            records={records}
            save={(s) => void saveSchema(s)}
          />
        </Popover>
      )}
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          items={[
            { label: 'Rename view', icon: 'edit', onSelect: () => setRenaming(menu.view.id) },
            ...(schema.views.length > 1
              ? [
                  {
                    label: 'Delete view',
                    icon: 'trash' as const,
                    danger: true,
                    onSelect: () => removeView(menu.view),
                  },
                ]
              : []),
          ]}
        />
      )}

      <Table
        columns={columns}
        visible={visible}
        rows={rows}
        total={records.length}
        setValue={setValue}
        onNew={() => void actions.createPage(page.id, { select: false })}
        filtered={filtered}
      />
    </article>
  );
}

function DbTitle({ page }: { page: Page }) {
  const [value, setValue] = useState(page.title);
  const saved = useRef(page.title);
  useEffect(() => {
    saved.current = page.title;
    setValue(page.title);
  }, [page.title]);
  const commit = () => {
    if (value !== saved.current) {
      saved.current = value;
      void actions.rename(page.id, value);
    }
  };
  return (
    <div className="title-row">
      <span className="title-icon" aria-hidden="true">
        {page.icon ?? <Icon name="table" size={30} />}
      </span>
      <textarea
        className="title"
        rows={1}
        value={value}
        placeholder="Untitled database"
        aria-label="Database title"
        onChange={(e) => setValue(e.target.value.replace(/\n/g, ' '))}
        onBlur={commit}
        onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), e.currentTarget.blur())}
      />
    </div>
  );
}

function Table({
  columns,
  visible,
  rows,
  total,
  setValue,
  onNew,
  filtered,
}: {
  columns: string;
  visible: Property[];
  rows: DbRecord[];
  total: number;
  setValue: (pageId: string, propertyId: string, value: unknown) => void;
  onNew: () => void;
  filtered: boolean;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  // Header and (absolutely positioned) rows must agree on width, so both get it explicitly.
  const minWidth = 240 + visible.length * 150;
  const [range, setRange] = useState({ start: 0, end: 40 });

  // Virtualized rows: only the visible window is rendered, so tables with thousands of records
  // stay responsive.
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const update = () => {
      const start = Math.max(0, Math.floor(el.scrollTop / ROW_H) - OVERSCAN);
      const end = Math.min(
        rows.length,
        Math.ceil((el.scrollTop + el.clientHeight) / ROW_H) + OVERSCAN,
      );
      setRange((r) => (r.start === start && r.end === end ? r : { start, end }));
    };
    update();
    el.addEventListener('scroll', update, { passive: true });
    const ro = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(update);
    ro?.observe(el);
    return () => {
      el.removeEventListener('scroll', update);
      ro?.disconnect();
    };
  }, [rows.length]);

  return (
    <div
      className="db-table"
      role="grid"
      aria-rowcount={rows.length + 1}
      aria-colcount={visible.length + 1}
    >
      <div className="db-scroll" ref={scroller}>
        <div
          className="db-grid db-header"
          role="row"
          style={{ gridTemplateColumns: columns, minWidth }}
        >
          <div role="columnheader" className="db-th">
            Name
          </div>
          {visible.map((p) => (
            <div key={p.id} role="columnheader" className="db-th">
              {p.name}
            </div>
          ))}
        </div>
        <div className="db-body" style={{ height: rows.length * ROW_H, minWidth }}>
          {rows.slice(range.start, range.end).map((r, i) => {
            const index = range.start + i;
            return (
              <div
                key={r.page.id}
                role="row"
                aria-rowindex={index + 2}
                className="db-grid db-row"
                style={{ gridTemplateColumns: columns, top: index * ROW_H, height: ROW_H }}
              >
                <div role="gridcell" className="db-td db-title-cell">
                  <TitleCell record={r} />
                </div>
                {visible.map((p) => (
                  <div key={p.id} role="gridcell" className="db-td">
                    <Cell
                      prop={p}
                      value={r.values[p.id]}
                      onChange={(v) => setValue(r.page.id, p.id, v)}
                    />
                  </div>
                ))}
              </div>
            );
          })}
        </div>
        {rows.length === 0 && (
          <div className="db-empty">
            {total === 0 ? (
              <>
                <p>No records yet.</p>
                <button className="btn btn-primary" onClick={onNew}>
                  New record
                </button>
              </>
            ) : (
              <p>{filtered ? 'No records match the filters.' : 'Nothing to show.'}</p>
            )}
          </div>
        )}
      </div>
      <div className="db-foot" role="status">
        {filtered && rows.length !== total
          ? `${rows.length} of ${total} records`
          : `${total} record${total === 1 ? '' : 's'}`}
      </div>
    </div>
  );
}

function TitleCell({ record }: { record: DbRecord }) {
  const [value, setValue] = useState(record.page.title);
  const focused = useRef(false);
  useEffect(() => {
    if (!focused.current) setValue(record.page.title);
  }, [record.page.title]);
  const commit = () => {
    focused.current = false;
    if (value !== record.page.title) void actions.rename(record.page.id, value);
  };
  return (
    <>
      <input
        className="cell-input cell-title"
        aria-label="Name"
        placeholder="Untitled"
        value={value}
        onFocus={() => (focused.current = true)}
        onChange={(e) => setValue(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
      />
      <button
        className="row-open"
        aria-label={`Open ${pageLabel(record.page)}`}
        title="Open"
        onClick={() => actions.open(record.page.id)}
      >
        Open
      </button>
      <button
        className="icon-btn row-del"
        aria-label={`Archive ${pageLabel(record.page)}`}
        title="Move to archive"
        onClick={() => void actions.archive(record.page.id)}
      >
        <Icon name="archive" size={14} />
      </button>
    </>
  );
}
