import type { Page } from './types';

/**
 * Database model (mirrors `crates/domain/src/database.rs`) and the client-side query engine.
 *
 * Filtering and sorting run here, not in storage: under end-to-end encryption the server cannot
 * evaluate them, so the same code must work on decrypted data on every client.
 */

export const SCHEMA_VERSION = 1;
export const TITLE_PROPERTY = 'title';

export type PropertyKind =
  'text' | 'number' | 'checkbox' | 'select' | 'multiSelect' | 'date' | 'url' | 'relation';

export const PROPERTY_KINDS: { kind: PropertyKind; label: string }[] = [
  { kind: 'text', label: 'Text' },
  { kind: 'number', label: 'Number' },
  { kind: 'select', label: 'Select' },
  { kind: 'multiSelect', label: 'Multi-select' },
  { kind: 'date', label: 'Date' },
  { kind: 'checkbox', label: 'Checkbox' },
  { kind: 'url', label: 'URL' },
  { kind: 'relation', label: 'Relation' },
];

export interface SelectOption {
  id: string;
  name: string;
  color?: string;
}

export interface Property {
  id: string;
  name: string;
  type: PropertyKind;
  options?: SelectOption[];
  relationDatabase?: string;
}

export type FilterOp =
  | 'contains'
  | 'notContains'
  | 'equals'
  | 'notEquals'
  | 'gt'
  | 'gte'
  | 'lt'
  | 'lte'
  | 'isEmpty'
  | 'isNotEmpty';

export interface Sort {
  property: string;
  direction: 'asc' | 'desc';
}

export interface Filter {
  property: string;
  op: FilterOp;
  value?: unknown;
}

export interface View {
  id: string;
  name: string;
  sorts: Sort[];
  filters: Filter[];
  filterMode: 'and' | 'or';
  hidden: string[];
}

export interface DatabaseSchema {
  version: number;
  properties: Property[];
  views: View[];
}

/** A record (a page inside a database) with its non-empty property values. */
export interface DbRecord {
  page: Page;
  values: Record<string, unknown>;
}

export function defaultSchema(): DatabaseSchema {
  return {
    version: SCHEMA_VERSION,
    properties: [],
    views: [
      { id: 'default', name: 'Table', sorts: [], filters: [], filterMode: 'and', hidden: [] },
    ],
  };
}

// ---- values -------------------------------------------------------------------------------------

const isIsoDate = (s: string) => {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s);
  if (!m) return false;
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const dt = new Date(Date.UTC(y, mo - 1, d));
  return dt.getUTCFullYear() === y && dt.getUTCMonth() === mo - 1 && dt.getUTCDate() === d;
};

/** Returns an error message, or null if `value` fits `prop`. `null` (empty) always fits. */
export function validateValue(prop: Property, value: unknown): string | null {
  if (value === null || value === undefined) return null;
  const bad = (what: string) => `“${prop.name}” expects ${what}`;
  const hasOption = (v: unknown) =>
    typeof v === 'string' && !!prop.options?.some((o) => o.id === v);
  switch (prop.type) {
    case 'text':
      return typeof value === 'string' ? null : bad('text');
    case 'url':
      return typeof value === 'string' &&
        (value === '' || value.includes('://') || value.startsWith('mailto:'))
        ? null
        : bad('a link such as https://…');
    case 'number':
      return typeof value === 'number' && Number.isFinite(value) ? null : bad('a number');
    case 'checkbox':
      return typeof value === 'boolean' ? null : bad('true or false');
    case 'select':
      return hasOption(value) ? null : bad('one of its options');
    case 'multiSelect':
      return Array.isArray(value) && value.every(hasOption) ? null : bad('a list of its options');
    case 'date':
      return typeof value === 'string' && isIsoDate(value) ? null : bad('a date (YYYY-MM-DD)');
    case 'relation':
      return Array.isArray(value) && value.every((v) => typeof v === 'string')
        ? null
        : bad('a list of pages');
  }
}

export const isEmptyValue = (v: unknown): boolean =>
  v === null || v === undefined || v === '' || (Array.isArray(v) && v.length === 0);

/** Filter operators that make sense for a property type, in display order. */
export function opsFor(kind: PropertyKind | 'title'): { op: FilterOp; label: string }[] {
  const empty = [
    { op: 'isEmpty' as const, label: 'is empty' },
    { op: 'isNotEmpty' as const, label: 'is not empty' },
  ];
  switch (kind) {
    case 'title':
    case 'text':
    case 'url':
      return [
        { op: 'contains', label: 'contains' },
        { op: 'notContains', label: 'does not contain' },
        { op: 'equals', label: 'is' },
        { op: 'notEquals', label: 'is not' },
        ...empty,
      ];
    case 'number':
      return [
        { op: 'equals', label: '=' },
        { op: 'notEquals', label: '≠' },
        { op: 'gt', label: '>' },
        { op: 'gte', label: '≥' },
        { op: 'lt', label: '<' },
        { op: 'lte', label: '≤' },
        ...empty,
      ];
    case 'date':
      return [
        { op: 'equals', label: 'is' },
        { op: 'lt', label: 'is before' },
        { op: 'gt', label: 'is after' },
        ...empty,
      ];
    case 'checkbox':
      return [{ op: 'equals', label: 'is' }];
    case 'select':
      return [{ op: 'equals', label: 'is' }, { op: 'notEquals', label: 'is not' }, ...empty];
    case 'multiSelect':
    case 'relation':
      return [
        { op: 'contains', label: 'contains' },
        { op: 'notContains', label: 'does not contain' },
        ...empty,
      ];
  }
}

// ---- query engine -------------------------------------------------------------------------------

const collator = new Intl.Collator(undefined, { sensitivity: 'base', numeric: true });

function cellOf(rec: DbRecord, property: string): unknown {
  return property === TITLE_PROPERTY ? rec.page.title : rec.values[property];
}

function kindOf(schema: DatabaseSchema, property: string): PropertyKind | 'title' {
  return property === TITLE_PROPERTY
    ? 'title'
    : (schema.properties.find((p) => p.id === property)?.type ?? 'text');
}

function compareValues(
  kind: PropertyKind | 'title',
  a: unknown,
  b: unknown,
  property?: Property,
): number {
  const ae = isEmptyValue(a);
  const be = isEmptyValue(b);
  if (ae || be) return ae === be ? 0 : ae ? 1 : -1; // empties sink in ascending order
  switch (kind) {
    case 'number':
      return (a as number) - (b as number);
    case 'checkbox':
      return Number(Boolean(a)) - Number(Boolean(b));
    case 'date':
      return String(a) < String(b) ? -1 : String(a) > String(b) ? 1 : 0;
    case 'select': {
      // Options sort in the order they are defined, not alphabetically by id or name.
      const idx = (v: unknown) => property?.options?.findIndex((o) => o.id === v) ?? -1;
      return idx(a) - idx(b);
    }
    case 'multiSelect':
    case 'relation':
      return (a as unknown[]).length - (b as unknown[]).length;
    default:
      return collator.compare(String(a), String(b));
  }
}

function matches(rec: DbRecord, filter: Filter, schema: DatabaseSchema): boolean {
  const kind = kindOf(schema, filter.property);
  const v = cellOf(rec, filter.property);
  const f = filter.value;
  const empty = isEmptyValue(v);
  switch (filter.op) {
    case 'isEmpty':
      return kind === 'checkbox' ? v !== true : empty;
    case 'isNotEmpty':
      return kind === 'checkbox' ? v === true : !empty;
    case 'contains':
    case 'notContains': {
      let hit: boolean;
      if (Array.isArray(v)) hit = v.includes(f);
      else
        hit = String(v ?? '')
          .toLowerCase()
          .includes(String(f ?? '').toLowerCase());
      return filter.op === 'contains' ? hit : !hit;
    }
    case 'equals':
    case 'notEquals': {
      let eq: boolean;
      if (kind === 'checkbox') eq = (v === true) === (f === true);
      else if (kind === 'text' || kind === 'title' || kind === 'url') {
        eq = String(v ?? '').toLowerCase() === String(f ?? '').toLowerCase();
      } else eq = !empty && v === f;
      return filter.op === 'equals' ? eq : !eq;
    }
    default: {
      // Ordering comparisons never match empty cells or an unset operand.
      if (empty || f === undefined || f === null || f === '') return false;
      const c = compareValues(kind, v, f);
      return filter.op === 'gt'
        ? c > 0
        : filter.op === 'gte'
          ? c >= 0
          : filter.op === 'lt'
            ? c < 0
            : c <= 0;
    }
  }
}

/** Applies a view's filters and sorts. Pure: never mutates its inputs. Stable for equal keys. */
export function applyView(records: DbRecord[], schema: DatabaseSchema, view: View): DbRecord[] {
  const active = view.filters.filter(
    (f) => f.property === TITLE_PROPERTY || schema.properties.some((p) => p.id === f.property),
  );
  let out = records;
  if (active.length) {
    out = records.filter((r) =>
      view.filterMode === 'or'
        ? active.some((f) => matches(r, f, schema))
        : active.every((f) => matches(r, f, schema)),
    );
  }
  const sorts = view.sorts.filter(
    (s) => s.property === TITLE_PROPERTY || schema.properties.some((p) => p.id === s.property),
  );
  if (!sorts.length) return out === records ? [...records] : out;
  return out
    .map((r, i) => ({ r, i }))
    .sort((x, y) => {
      for (const s of sorts) {
        const kind = kindOf(schema, s.property);
        const a = cellOf(x.r, s.property);
        const b = cellOf(y.r, s.property);
        let c = compareValues(
          kind,
          a,
          b,
          schema.properties.find((p) => p.id === s.property),
        );
        // Empty cells stay last regardless of direction.
        if (isEmptyValue(a) || isEmptyValue(b)) {
          if (c !== 0) return c;
          continue;
        }
        if (s.direction === 'desc') c = -c;
        if (c !== 0) return c;
      }
      return x.i - y.i;
    })
    .map((x) => x.r);
}

// ---- schema editing helpers ---------------------------------------------------------------------

export function newKey(prefix: string): string {
  return `${prefix}_${Math.random().toString(36).slice(2, 8)}`;
}

/** Display text of a cell, for tooltips, exports and search. */
export function displayValue(
  prop: Property,
  value: unknown,
  pageTitle?: (id: string) => string,
): string {
  if (isEmptyValue(value)) return '';
  switch (prop.type) {
    case 'select':
      return prop.options?.find((o) => o.id === value)?.name ?? '';
    case 'multiSelect':
      return (value as string[])
        .map((id) => prop.options?.find((o) => o.id === id)?.name ?? '')
        .filter(Boolean)
        .join(', ');
    case 'relation':
      return (value as string[])
        .map((id) => pageTitle?.(id) ?? '')
        .filter(Boolean)
        .join(', ');
    case 'checkbox':
      return value ? '✓' : '';
    default:
      return String(value);
  }
}
