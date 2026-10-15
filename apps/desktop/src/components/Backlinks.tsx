import { useEffect, useState } from 'react';
import type { Page } from '@notercrux/document-model';
import { api } from '../api';
import { actions, pageLabel, useApp } from '../state/app';
import { Icon } from './Icon';

/** "Linked from" list: pages whose content links to this one. */
export function Backlinks({ pageId }: { pageId: string }) {
  const pages = useApp((s) => s.pages);
  const [links, setLinks] = useState<Page[]>([]);

  useEffect(() => {
    let cancelled = false;
    api
      .backlinks(pageId)
      .then((l) => !cancelled && setLinks(l))
      .catch(() => !cancelled && setLinks([]));
    return () => {
      cancelled = true;
    };
  }, [pageId, pages]);

  if (!links.length) return null;
  return (
    <section className="backlinks" aria-label="Linked from">
      <h3>Linked from</h3>
      <ul>
        {links.map((p) => (
          <li key={p.id}>
            <button className="crumb" onClick={() => actions.open(p.id)}>
              <Icon name="link" size={13} /> {p.icon} {pageLabel(p)}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
