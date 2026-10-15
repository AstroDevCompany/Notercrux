import { useCallback, useEffect, useRef, useState } from 'react';
import type { DatabaseSchema, DbRecord, View } from '@notercrux/document-model';
import { api } from '../../api';
import { actions, useApp } from '../../state/app';

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * Loads a database's schema and records and exposes optimistic edits. Failed writes are reported
 * and the view is reloaded from storage so the screen never shows a value that was not saved.
 */
export function useDatabase(dbId: string) {
  const pages = useApp((s) => s.pages); // renames/archives/new records elsewhere trigger a reload
  const [schema, setSchema] = useState<DatabaseSchema | null>(null);
  const [records, setRecords] = useState<DbRecord[]>([]);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);

  const reload = useCallback(async () => {
    try {
      const [s, r] = await Promise.all([api.getSchema(dbId), api.listRecords(dbId)]);
      if (!alive.current) return;
      setSchema(s);
      setRecords(r);
      setError(null);
    } catch (e) {
      if (alive.current) setError(msg(e));
    }
  }, [dbId]);

  useEffect(() => {
    alive.current = true;
    void reload();
    return () => {
      alive.current = false;
    };
  }, [reload, pages]);

  const fail = useCallback(
    (e: unknown) => {
      actions.notify(msg(e));
      void reload();
    },
    [reload],
  );

  const saveSchema = useCallback(
    async (next: DatabaseSchema) => {
      setSchema(next);
      try {
        await api.setSchema(dbId, next);
        await reload(); // the backend may have cleared values that no longer fit
      } catch (e) {
        fail(e);
      }
    },
    [dbId, reload, fail],
  );

  const setValue = useCallback(
    async (pageId: string, propertyId: string, value: unknown) => {
      setRecords((rs) =>
        rs.map((r) => {
          if (r.page.id !== pageId) return r;
          const values = { ...r.values };
          if (value === null || value === '' || (Array.isArray(value) && !value.length))
            delete values[propertyId];
          else values[propertyId] = value;
          return { ...r, values };
        }),
      );
      try {
        await api.setProperty(pageId, propertyId, value);
      } catch (e) {
        fail(e);
      }
    },
    [fail],
  );

  const updateView = useCallback(
    (viewId: string, patch: Partial<View>) => {
      if (!schema) return Promise.resolve();
      return saveSchema({
        ...schema,
        views: schema.views.map((v) => (v.id === viewId ? { ...v, ...patch } : v)),
      });
    },
    [schema, saveSchema],
  );

  return { schema, records, error, reload, saveSchema, setValue, updateView };
}
