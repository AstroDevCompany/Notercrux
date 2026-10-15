import { useCallback, useEffect, useState } from 'react';
import type { DatabaseSchema, Page } from '@notercrux/document-model';
import { api } from '../../api';
import { actions, pageLabel, useApp } from '../../state/app';
import { Cell } from './Cells';

/** Property editor shown above the content of a page that is a record of a database. */
export function RecordProperties({ page }: { page: Page }) {
  const pages = useApp((s) => s.pages);
  const parent = pages.find((p) => p.id === page.parentId);
  const dbId = parent?.kind === 'database' ? parent.id : null;
  const [schema, setSchema] = useState<DatabaseSchema | null>(null);
  const [values, setValues] = useState<Record<string, unknown>>({});

  const load = useCallback(async () => {
    if (!dbId) return;
    try {
      const [s, v] = await Promise.all([api.getSchema(dbId), api.getProperties(page.id)]);
      setSchema(s);
      setValues(v);
    } catch (e) {
      actions.notify(e instanceof Error ? e.message : String(e));
    }
  }, [dbId, page.id]);

  useEffect(() => {
    void load();
  }, [load]);

  if (!dbId || !schema || !schema.properties.length) return null;

  const set = async (propertyId: string, value: unknown) => {
    setValues((v) => {
      const next = { ...v };
      if (value === null || value === '' || (Array.isArray(value) && !value.length))
        delete next[propertyId];
      else next[propertyId] = value;
      return next;
    });
    try {
      await api.setProperty(page.id, propertyId, value);
    } catch (e) {
      actions.notify(e instanceof Error ? e.message : String(e));
      void load(); // show what is actually stored
    }
  };

  return (
    <section className="record-props" aria-label="Properties">
      <button className="record-db crumb" onClick={() => actions.open(dbId)}>
        in {pageLabel(parent!)}
      </button>
      <dl>
        {schema.properties.map((p) => (
          <div key={p.id} className="record-prop">
            <dt>{p.name}</dt>
            <dd>
              <Cell prop={p} value={values[p.id]} onChange={(v) => void set(p.id, v)} />
            </dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
