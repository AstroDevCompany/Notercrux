import { useEffect, useMemo, useRef, useState } from 'react';
import type { DbRecord, Property } from '@notercrux/document-model';
import { api } from '../../api';
import { actions, pageLabel, useApp } from '../../state/app';
import { Popover } from './Popover';

interface CellProps {
  prop: Property;
  value: unknown;
  onChange: (value: unknown) => void;
}

/** Editor for one property value. Used by table cells and by the record properties panel. */
export function Cell({ prop, value, onChange }: CellProps) {
  switch (prop.type) {
    case 'text':
    case 'url':
      return <TextInput prop={prop} value={value} onChange={onChange} />;
    case 'number':
      return <NumberInput prop={prop} value={value} onChange={onChange} />;
    case 'checkbox':
      return (
        <input
          type="checkbox"
          className="cell-check"
          aria-label={prop.name}
          checked={value === true}
          onChange={(e) => onChange(e.target.checked ? true : null)}
        />
      );
    case 'date':
      return (
        <input
          type="date"
          className="cell-input"
          aria-label={prop.name}
          value={typeof value === 'string' ? value : ''}
          onChange={(e) => onChange(e.target.value || null)}
        />
      );
    case 'select':
      return (
        <select
          className="cell-input cell-select"
          aria-label={prop.name}
          value={typeof value === 'string' ? value : ''}
          onChange={(e) => onChange(e.target.value || null)}
        >
          <option value="" />
          {(prop.options ?? []).map((o) => (
            <option key={o.id} value={o.id}>
              {o.name}
            </option>
          ))}
        </select>
      );
    case 'multiSelect':
      return <MultiSelect prop={prop} value={value} onChange={onChange} />;
    case 'relation':
      return <Relation prop={prop} value={value} onChange={onChange} />;
  }
}

function TextInput({ prop, value, onChange }: CellProps) {
  const [draft, setDraft] = useState(typeof value === 'string' ? value : '');
  const focused = useRef(false);
  useEffect(() => {
    if (!focused.current) setDraft(typeof value === 'string' ? value : '');
  }, [value]);
  const commit = () => {
    focused.current = false;
    const current = typeof value === 'string' ? value : '';
    if (draft !== current) onChange(draft === '' ? null : draft);
  };
  return (
    <input
      className="cell-input"
      aria-label={prop.name}
      inputMode={prop.type === 'url' ? 'url' : undefined}
      value={draft}
      onFocus={() => (focused.current = true)}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur();
        if (e.key === 'Escape') {
          setDraft(typeof value === 'string' ? value : '');
          e.currentTarget.blur();
        }
      }}
    />
  );
}

function NumberInput({ prop, value, onChange }: CellProps) {
  const text = typeof value === 'number' ? String(value) : '';
  const [draft, setDraft] = useState(text);
  const focused = useRef(false);
  useEffect(() => {
    if (!focused.current) setDraft(text);
  }, [text]);
  const commit = () => {
    focused.current = false;
    if (draft.trim() === '') return draft !== text ? onChange(null) : undefined;
    const n = Number(draft);
    if (!Number.isFinite(n)) return setDraft(text); // reject garbage instead of saving it
    if (String(n) !== text) onChange(n);
  };
  return (
    <input
      className="cell-input cell-number"
      aria-label={prop.name}
      inputMode="decimal"
      value={draft}
      onFocus={() => (focused.current = true)}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
    />
  );
}

function Chips({ names, placeholder }: { names: string[]; placeholder: string }) {
  if (!names.length) return <span className="chip-placeholder">{placeholder}</span>;
  return (
    <span className="chips">
      {names.map((n, i) => (
        <span key={i} className="chip">
          {n}
        </span>
      ))}
    </span>
  );
}

function MultiSelect({ prop, value, onChange }: CellProps) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const selected = Array.isArray(value) ? (value as string[]) : [];
  const names = selected
    .map((id) => prop.options?.find((o) => o.id === id)?.name)
    .filter((n): n is string => !!n);
  const toggle = (id: string) =>
    onChange(selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id]);
  return (
    <>
      <button
        ref={anchor}
        className="cell-pick"
        aria-label={prop.name}
        aria-haspopup="dialog"
        onClick={() => setOpen(!open)}
      >
        <Chips names={names} placeholder="" />
      </button>
      {open && (
        <Popover anchor={anchor} label={prop.name} width={240} onClose={() => setOpen(false)}>
          <div className="pop-list">
            {(prop.options ?? []).length === 0 && (
              <div className="pop-empty">No options yet. Add them in Properties.</div>
            )}
            {(prop.options ?? []).map((o) => (
              <label key={o.id} className="pop-check">
                <input
                  type="checkbox"
                  checked={selected.includes(o.id)}
                  onChange={() => toggle(o.id)}
                />
                {o.name}
              </label>
            ))}
          </div>
        </Popover>
      )}
    </>
  );
}

function Relation({ prop, value, onChange }: CellProps) {
  const pages = useApp((s) => s.pages);
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const selected = Array.isArray(value) ? (value as string[]) : [];
  const titleOf = (id: string) => {
    const p = pages.find((x) => x.id === id && !x.archivedAt);
    return p ? pageLabel(p) : null; // deleted or archived targets are hidden, not shown as ghosts
  };
  const names = selected.map(titleOf).filter((n): n is string => n !== null);
  return (
    <>
      <button
        ref={anchor}
        className="cell-pick"
        aria-label={prop.name}
        aria-haspopup="dialog"
        onClick={() => setOpen(!open)}
      >
        <Chips names={names} placeholder="" />
      </button>
      {open && (
        <Popover
          anchor={anchor}
          label={`${prop.name} relation`}
          width={300}
          onClose={() => setOpen(false)}
        >
          <RelationPicker target={prop.relationDatabase} selected={selected} onChange={onChange} />
        </Popover>
      )}
    </>
  );
}

function RelationPicker({
  target,
  selected,
  onChange,
}: {
  target?: string;
  selected: string[];
  onChange: (v: unknown) => void;
}) {
  const [records, setRecords] = useState<DbRecord[] | null>(null);
  const [q, setQ] = useState('');
  useEffect(() => {
    if (!target) return setRecords([]);
    api
      .listRecords(target)
      .then(setRecords)
      .catch(() => setRecords([]));
  }, [target]);
  const shown = useMemo(
    () => (records ?? []).filter((r) => pageLabel(r.page).toLowerCase().includes(q.toLowerCase())),
    [records, q],
  );
  const toggle = (id: string) =>
    onChange(selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id]);
  return (
    <>
      <input
        className="pop-search"
        data-autofocus
        placeholder="Search records…"
        aria-label="Search records"
        value={q}
        onChange={(e) => setQ(e.target.value)}
      />
      <div className="pop-list">
        {records === null && <div className="pop-empty">Loading…</div>}
        {records && !shown.length && (
          <div className="pop-empty">
            {records.length ? 'No matches' : 'The related database has no records.'}
          </div>
        )}
        {shown.map((r) => (
          <label key={r.page.id} className="pop-check">
            <input
              type="checkbox"
              checked={selected.includes(r.page.id)}
              onChange={() => toggle(r.page.id)}
            />
            {pageLabel(r.page)}
          </label>
        ))}
      </div>
    </>
  );
}

/** Opens a record page from a table or relation chip. */
export const openRecord = (id: string) => actions.open(id);
