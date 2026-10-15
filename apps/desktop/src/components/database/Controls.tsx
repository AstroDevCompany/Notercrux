import { useEffect, useState } from 'react';
import {
  newKey,
  opsFor,
  PROPERTY_KINDS,
  TITLE_PROPERTY,
  type DatabaseSchema,
  type DbRecord,
  type Filter,
  type Property,
  type PropertyKind,
  type Sort,
  type View,
} from '@notercrux/document-model';
import { api } from '../../api';
import { actions, live, pageLabel, useApp } from '../../state/app';
import { Icon } from '../Icon';

type Update = (patch: Partial<View>) => void;

const propChoices = (schema: DatabaseSchema) => [
  { id: TITLE_PROPERTY, name: 'Name', kind: 'title' as const },
  ...schema.properties.map((p) => ({ id: p.id, name: p.name, kind: p.type })),
];

// ---- filters ------------------------------------------------------------------------------------

function RelationOptions({ target }: { target?: string }) {
  const [records, setRecords] = useState<DbRecord[]>([]);
  useEffect(() => {
    if (target)
      api
        .listRecords(target)
        .then(setRecords)
        .catch(() => setRecords([]));
  }, [target]);
  return (
    <>
      <option value="" />
      {records.map((r) => (
        <option key={r.page.id} value={r.page.id}>
          {pageLabel(r.page)}
        </option>
      ))}
    </>
  );
}

function FilterValue({
  filter,
  prop,
  onChange,
}: {
  filter: Filter;
  prop?: Property;
  onChange: (v: unknown) => void;
}) {
  if (filter.op === 'isEmpty' || filter.op === 'isNotEmpty') return <span />;
  const kind = prop?.type ?? 'text';
  const v = filter.value;
  switch (kind) {
    case 'number':
      return (
        <input
          className="pop-input"
          aria-label="Filter value"
          inputMode="decimal"
          value={typeof v === 'number' ? String(v) : ''}
          onChange={(e) =>
            onChange(
              e.target.value === '' || !Number.isFinite(Number(e.target.value))
                ? null
                : Number(e.target.value),
            )
          }
        />
      );
    case 'date':
      return (
        <input
          type="date"
          className="pop-input"
          aria-label="Filter value"
          value={typeof v === 'string' ? v : ''}
          onChange={(e) => onChange(e.target.value || null)}
        />
      );
    case 'checkbox':
      return (
        <select
          className="pop-input"
          aria-label="Filter value"
          value={v === false ? 'false' : 'true'}
          onChange={(e) => onChange(e.target.value === 'true')}
        >
          <option value="true">checked</option>
          <option value="false">unchecked</option>
        </select>
      );
    case 'select':
    case 'multiSelect':
      return (
        <select
          className="pop-input"
          aria-label="Filter value"
          value={typeof v === 'string' ? v : ''}
          onChange={(e) => onChange(e.target.value || null)}
        >
          <option value="" />
          {(prop?.options ?? []).map((o) => (
            <option key={o.id} value={o.id}>
              {o.name}
            </option>
          ))}
        </select>
      );
    case 'relation':
      return (
        <select
          className="pop-input"
          aria-label="Filter value"
          value={typeof v === 'string' ? v : ''}
          onChange={(e) => onChange(e.target.value || null)}
        >
          <RelationOptions target={prop?.relationDatabase} />
        </select>
      );
    default:
      return (
        <input
          className="pop-input"
          aria-label="Filter value"
          value={typeof v === 'string' ? v : ''}
          onChange={(e) => onChange(e.target.value)}
        />
      );
  }
}

export function FilterEditor({
  schema,
  view,
  update,
}: {
  schema: DatabaseSchema;
  view: View;
  update: Update;
}) {
  const choices = propChoices(schema);
  const setFilter = (i: number, patch: Partial<Filter>) =>
    update({ filters: view.filters.map((f, k) => (k === i ? { ...f, ...patch } : f)) });
  return (
    <div className="pop-body">
      {view.filters.length === 0 && (
        <div className="pop-empty">No filters. Add one to narrow this view.</div>
      )}
      {view.filters.map((f, i) => {
        const prop = schema.properties.find((p) => p.id === f.property);
        const kind: PropertyKind | 'title' =
          f.property === TITLE_PROPERTY ? 'title' : (prop?.type ?? 'text');
        const ops = opsFor(kind);
        return (
          <div key={i} className="pop-row">
            <select
              className="pop-input"
              aria-label="Filter property"
              value={f.property}
              onChange={(e) => {
                const next = choices.find((c) => c.id === e.target.value)!;
                setFilter(i, { property: next.id, op: opsFor(next.kind)[0]!.op, value: undefined });
              }}
            >
              {choices.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
            <select
              className="pop-input"
              aria-label="Filter operator"
              value={f.op}
              onChange={(e) => setFilter(i, { op: e.target.value as Filter['op'] })}
            >
              {ops.map((o) => (
                <option key={o.op} value={o.op}>
                  {o.label}
                </option>
              ))}
            </select>
            <FilterValue filter={f} prop={prop} onChange={(value) => setFilter(i, { value })} />
            <button
              className="icon-btn"
              aria-label="Remove filter"
              onClick={() => update({ filters: view.filters.filter((_, k) => k !== i) })}
            >
              <Icon name="x" size={14} />
            </button>
          </div>
        );
      })}
      <div className="pop-foot">
        <button
          className="btn"
          onClick={() =>
            update({
              filters: [...view.filters, { property: TITLE_PROPERTY, op: 'contains', value: '' }],
            })
          }
        >
          Add filter
        </button>
        {view.filters.length > 1 && (
          <label className="pop-inline">
            Match
            <select
              className="pop-input"
              aria-label="Combine filters"
              value={view.filterMode}
              onChange={(e) => update({ filterMode: e.target.value as View['filterMode'] })}
            >
              <option value="and">all filters</option>
              <option value="or">any filter</option>
            </select>
          </label>
        )}
      </div>
    </div>
  );
}

// ---- sorts --------------------------------------------------------------------------------------

export function SortEditor({
  schema,
  view,
  update,
}: {
  schema: DatabaseSchema;
  view: View;
  update: Update;
}) {
  const choices = propChoices(schema);
  const setSort = (i: number, patch: Partial<Sort>) =>
    update({ sorts: view.sorts.map((s, k) => (k === i ? { ...s, ...patch } : s)) });
  const unused = choices.find((c) => !view.sorts.some((s) => s.property === c.id));
  return (
    <div className="pop-body">
      {view.sorts.length === 0 && (
        <div className="pop-empty">No sorting. Records appear in their manual order.</div>
      )}
      {view.sorts.map((s, i) => (
        <div key={i} className="pop-row">
          <span className="pop-rank">{i === 0 ? 'Sort by' : 'then by'}</span>
          <select
            className="pop-input"
            aria-label="Sort property"
            value={s.property}
            onChange={(e) => setSort(i, { property: e.target.value })}
          >
            {choices.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
          <select
            className="pop-input"
            aria-label="Sort direction"
            value={s.direction}
            onChange={(e) => setSort(i, { direction: e.target.value as Sort['direction'] })}
          >
            <option value="asc">ascending</option>
            <option value="desc">descending</option>
          </select>
          <button
            className="icon-btn"
            aria-label="Remove sort"
            onClick={() => update({ sorts: view.sorts.filter((_, k) => k !== i) })}
          >
            <Icon name="x" size={14} />
          </button>
        </div>
      ))}
      <div className="pop-foot">
        <button
          className="btn"
          disabled={!unused}
          onClick={() =>
            unused && update({ sorts: [...view.sorts, { property: unused.id, direction: 'asc' }] })
          }
        >
          Add sort
        </button>
      </div>
    </div>
  );
}

// ---- properties ---------------------------------------------------------------------------------

export function PropertiesEditor({
  dbId,
  schema,
  view,
  records,
  save,
}: {
  dbId: string;
  schema: DatabaseSchema;
  view: View;
  records: DbRecord[];
  save: (s: DatabaseSchema) => void;
}) {
  const pages = useApp((s) => s.pages);
  const databases = live(pages).filter((p) => p.kind === 'database');
  const [name, setName] = useState('');
  const [kind, setKind] = useState<PropertyKind>('text');
  const [target, setTarget] = useState(dbId);
  const [open, setOpen] = useState<string | null>(null);

  const patchProp = (id: string, patch: Partial<Property>) =>
    save({
      ...schema,
      properties: schema.properties.map((p) => (p.id === id ? { ...p, ...patch } : p)),
    });
  const hasValues = (id: string) => records.some((r) => r.values[id] !== undefined);

  const changeKind = (p: Property, next: PropertyKind) => {
    if (next === p.type) return;
    const apply = () =>
      patchProp(p.id, {
        type: next,
        options: next === 'select' || next === 'multiSelect' ? (p.options ?? []) : undefined,
        relationDatabase: next === 'relation' ? (databases[0]?.id ?? dbId) : undefined,
      });
    if (!hasValues(p.id)) return apply();
    actions.ask({
      title: `Change “${p.name}” to ${PROPERTY_KINDS.find((k) => k.kind === next)!.label}?`,
      message:
        'The values already in this column cannot be converted and will be cleared for every record.',
      confirmLabel: 'Change type',
      destructive: true,
      onConfirm: apply,
    });
  };

  const remove = (p: Property) => {
    const apply = () =>
      save({
        ...schema,
        properties: schema.properties.filter((x) => x.id !== p.id),
        // Views must not keep pointing at a property that no longer exists.
        views: schema.views.map((v) => ({
          ...v,
          sorts: v.sorts.filter((s) => s.property !== p.id),
          filters: v.filters.filter((f) => f.property !== p.id),
          hidden: v.hidden.filter((h) => h !== p.id),
        })),
      });
    if (!hasValues(p.id)) return apply();
    actions.ask({
      title: `Delete “${p.name}”?`,
      message: 'This removes the property and its values from every record in the database.',
      confirmLabel: 'Delete property',
      destructive: true,
      onConfirm: apply,
    });
  };

  const add = () => {
    const n = name.trim();
    if (!n) return;
    const prop: Property = {
      id: newKey('p'),
      name: n,
      type: kind,
      ...(kind === 'select' || kind === 'multiSelect' ? { options: [] } : {}),
      ...(kind === 'relation' ? { relationDatabase: target } : {}),
    };
    save({ ...schema, properties: [...schema.properties, prop] });
    setName('');
  };

  return (
    <div className="pop-body">
      {schema.properties.length === 0 && (
        <div className="pop-empty">Only the Name column so far.</div>
      )}
      {schema.properties.map((p) => {
        const hidden = view.hidden.includes(p.id);
        const isChoice = p.type === 'select' || p.type === 'multiSelect';
        return (
          <div key={p.id} className="pop-prop">
            <div className="pop-row">
              <input
                className="pop-input"
                aria-label="Property name"
                defaultValue={p.name}
                onBlur={(e) =>
                  e.target.value.trim() &&
                  e.target.value !== p.name &&
                  patchProp(p.id, { name: e.target.value.trim() })
                }
                onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
              />
              <select
                className="pop-input"
                aria-label={`Type of ${p.name}`}
                value={p.type}
                onChange={(e) => changeKind(p, e.target.value as PropertyKind)}
              >
                {PROPERTY_KINDS.map((k) => (
                  <option key={k.kind} value={k.kind}>
                    {k.label}
                  </option>
                ))}
              </select>
              <button
                className={`icon-btn${hidden ? ' is-dim' : ''}`}
                aria-label={hidden ? `Show ${p.name} in this view` : `Hide ${p.name} in this view`}
                aria-pressed={!hidden}
                title={hidden ? 'Hidden in this view' : 'Shown in this view'}
                onClick={() => {
                  const hiddenList = hidden
                    ? view.hidden.filter((h) => h !== p.id)
                    : [...view.hidden, p.id];
                  save({
                    ...schema,
                    views: schema.views.map((v) =>
                      v.id === view.id ? { ...v, hidden: hiddenList } : v,
                    ),
                  });
                }}
              >
                {hidden ? '◌' : '●'}
              </button>
              <button
                className="icon-btn"
                aria-label={`Delete ${p.name}`}
                onClick={() => remove(p)}
              >
                <Icon name="trash" size={14} />
              </button>
              {(isChoice || p.type === 'relation') && (
                <button
                  className="icon-btn"
                  aria-expanded={open === p.id}
                  aria-label={`Edit ${p.name} options`}
                  onClick={() => setOpen(open === p.id ? null : p.id)}
                >
                  <Icon name="chevron" size={13} className={open === p.id ? 'rot90' : ''} />
                </button>
              )}
            </div>
            {open === p.id && isChoice && (
              <div className="pop-options">
                {(p.options ?? []).map((o) => (
                  <div key={o.id} className="pop-row">
                    <input
                      className="pop-input"
                      aria-label="Option name"
                      defaultValue={o.name}
                      onBlur={(e) =>
                        e.target.value.trim() &&
                        e.target.value !== o.name &&
                        patchProp(p.id, {
                          options: (p.options ?? []).map((x) =>
                            x.id === o.id ? { ...x, name: e.target.value.trim() } : x,
                          ),
                        })
                      }
                      onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
                    />
                    <button
                      className="icon-btn"
                      aria-label={`Delete option ${o.name}`}
                      onClick={() =>
                        patchProp(p.id, { options: (p.options ?? []).filter((x) => x.id !== o.id) })
                      }
                    >
                      <Icon name="x" size={14} />
                    </button>
                  </div>
                ))}
                <OptionAdder
                  onAdd={(n) =>
                    patchProp(p.id, {
                      options: [...(p.options ?? []), { id: newKey('o'), name: n }],
                    })
                  }
                />
              </div>
            )}
            {open === p.id && p.type === 'relation' && (
              <div className="pop-options">
                <label className="pop-inline">
                  Related database
                  <select
                    className="pop-input"
                    aria-label="Related database"
                    value={p.relationDatabase ?? ''}
                    onChange={(e) => patchProp(p.id, { relationDatabase: e.target.value })}
                  >
                    {databases.map((d) => (
                      <option key={d.id} value={d.id}>
                        {pageLabel(d)}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
            )}
          </div>
        );
      })}
      <div className="pop-add">
        <div className="pop-row">
          <input
            className="pop-input"
            aria-label="New property name"
            placeholder="New property name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && add()}
          />
          <select
            className="pop-input"
            aria-label="New property type"
            value={kind}
            onChange={(e) => setKind(e.target.value as PropertyKind)}
          >
            {PROPERTY_KINDS.map((k) => (
              <option key={k.kind} value={k.kind}>
                {k.label}
              </option>
            ))}
          </select>
        </div>
        {kind === 'relation' && (
          <label className="pop-inline">
            Related database
            <select
              className="pop-input"
              aria-label="New relation target"
              value={target}
              onChange={(e) => setTarget(e.target.value)}
            >
              {databases.map((d) => (
                <option key={d.id} value={d.id}>
                  {pageLabel(d)}
                </option>
              ))}
            </select>
          </label>
        )}
        <div className="pop-foot">
          <button className="btn btn-primary" disabled={!name.trim()} onClick={add}>
            Add property
          </button>
        </div>
      </div>
    </div>
  );
}

function OptionAdder({ onAdd }: { onAdd: (name: string) => void }) {
  const [v, setV] = useState('');
  const go = () => {
    if (v.trim()) {
      onAdd(v.trim());
      setV('');
    }
  };
  return (
    <div className="pop-row">
      <input
        className="pop-input"
        aria-label="New option"
        placeholder="Add option"
        value={v}
        onChange={(e) => setV(e.target.value)}
        onKeyDown={(e) => e.key === 'Enter' && go()}
      />
      <button className="btn" onClick={go} disabled={!v.trim()}>
        Add
      </button>
    </div>
  );
}
