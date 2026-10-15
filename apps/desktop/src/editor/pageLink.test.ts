import { describe, expect, it, vi } from 'vitest';
import { Editor } from '@tiptap/core';
import { fromProseMirror, toProseMirror } from '@notercrux/document-model';

describe('page links', () => {
  it('are stored as pageLink marks that survive the document round trip', async () => {
    vi.resetModules();
    const { createExtensions } = await import('./extensions');
    const e = new Editor({
      extensions: createExtensions(),
      element: document.createElement('div'),
      content: '<p>see <a data-page-id="0190a000-0000-7000-8000-000000000001">Roadmap</a> now</p>',
    });
    const doc = fromProseMirror(e.getJSON() as never);
    expect(doc.blocks[0]).toMatchObject({
      text: 'see Roadmap now',
      marks: [
        {
          from: 4,
          to: 11,
          type: 'pageLink',
          attrs: { pageId: '0190a000-0000-7000-8000-000000000001' },
        },
      ],
    });
    // And back into an editor, with the mark intact.
    e.commands.setContent(toProseMirror(doc) as never);
    expect(e.getHTML()).toContain('data-page-id="0190a000-0000-7000-8000-000000000001"');
    e.destroy();
  });

  it('suggests pages for the picker: excludes the current page, ranks prefixes, offers to create', async () => {
    vi.resetModules();
    localStorage.clear();
    const { actions } = await import('../state/app');
    const { linkItems } = await import('./pageLink');
    await actions.init();
    const items = linkItems('read', null);
    expect(items[0]!.title).toBe('Reading list');
    expect(items.at(-1)).toMatchObject({ create: true, title: 'read' });
    expect(linkItems('Reading list', null).some((i) => i.create)).toBe(false); // exact match: no create entry
    const welcome = linkItems('welcome', null)[0]!;
    expect(linkItems('welcome', welcome.id).some((i) => i.id === welcome.id)).toBe(false);
    expect(linkItems('', null).some((i) => i.create)).toBe(false);
  });
});
