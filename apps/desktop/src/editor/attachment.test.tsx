import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Editor } from '@tiptap/core';
import { EditorContent, useEditor } from '@tiptap/react';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { fromProseMirror, toProseMirror, type Document } from '@notercrux/document-model';
import { createExtensions } from './extensions';
import { replaceContent, sameDocument } from './replaceContent';
import { filterSlashItems } from './slashMenu';

const HASH = 'c'.repeat(64);
const p = (text: string, id: string) => ({
  id,
  type: 'paragraph' as const,
  text,
});
const doc = (...blocks: Document['blocks']): Document => ({ version: 1, blocks });

const make = (d: Document) =>
  new Editor({
    extensions: createExtensions(),
    content: toProseMirror(d) as never,
    element: document.createElement('div'),
  });

beforeEach(() => {
  localStorage.clear();
  Element.prototype.scrollIntoView = vi.fn();
  URL.createObjectURL = vi.fn(() => 'blob:test');
  URL.revokeObjectURL = vi.fn();
});
afterEach(cleanup);

describe('image and file blocks in the editor', () => {
  const withAttachments = doc(
    p('before', 'p1'),
    {
      id: 'i1',
      type: 'image',
      text: 'A caption',
      attrs: { attachment: HASH, name: 'cat.png', mime: 'image/png', size: 42 },
    },
    {
      id: 'f1',
      type: 'file',
      text: '',
      attrs: { attachment: HASH, name: 'plan.pdf', mime: 'application/pdf', size: 7 },
    },
    p('after', 'p2'),
  );

  it('load into the schema and come back out unchanged, keeping their block ids', () => {
    const e = make(withAttachments);
    const back = fromProseMirror(e.getJSON() as never);
    expect(back.blocks.map((b) => [b.id, b.type])).toEqual([
      ['p1', 'paragraph'],
      ['i1', 'image'],
      ['f1', 'file'],
      ['p2', 'paragraph'],
    ]);
    expect(back.blocks[1]).toEqual(withAttachments.blocks[1]);
    expect(back.blocks[2]).toEqual(withAttachments.blocks[2]);
  });

  it('can be inserted with a command and get a block id', () => {
    const e = make(doc(p('x', 'p1')));
    e.commands.insertContentAt(e.state.doc.content.size, {
      type: 'file',
      attrs: { attachment: HASH, name: 'a.txt', mime: 'text/plain', size: 1 },
    });
    const out = fromProseMirror(e.getJSON() as never);
    const file = out.blocks.find((b) => b.type === 'file')!;
    expect(file.id).toBeTruthy();
    expect(file.attrs).toMatchObject({ attachment: HASH, name: 'a.txt' });
  });

  it('are offered by the slash menu', () => {
    expect(filterSlashItems('imag').map((i) => i.id)).toContain('image');
    expect(filterSlashItems('attach').map((i) => i.id)).toContain('file');
  });

  it('show a placeholder while the file is still arriving, then the picture', async () => {
    const { api } = await import('../api');
    const { mockSyncPatch } = await import('../api/mock.sync');
    void mockSyncPatch;
    const bytes = new Uint8Array([137, 80, 78, 71, 1, 2, 3]);
    const meta = await api.addAttachment({ name: 'pic.png', mime: 'image/png', bytes });
    const d = doc({
      id: 'i1',
      type: 'image',
      text: 'caption here',
      attrs: { attachment: meta.id, name: 'pic.png', mime: 'image/png', size: meta.size },
    });
    function Host({ d }: { d: Document }) {
      const editor = useEditor({
        extensions: createExtensions(() => null, { views: true }),
        content: toProseMirror(d) as never,
      });
      return <EditorContent editor={editor} />;
    }
    render(<Host d={d} />);
    const img = await waitFor(() => {
      const el = document.querySelector('.attachment-image img');
      expect(el).toBeTruthy();
      return el as HTMLImageElement;
    });
    expect(img.getAttribute('src')).toBe('blob:test');
    expect((screen.getByLabelText('Caption for pic.png') as HTMLInputElement).value).toBe(
      'caption here',
    );
  });

  it('say so when the file is not on this device yet', async () => {
    const d = doc({
      id: 'f1',
      type: 'file',
      text: '',
      attrs: {
        attachment: 'd'.repeat(64),
        name: 'report.pdf',
        mime: 'application/pdf',
        size: 2048,
      },
    });
    function Host() {
      const editor = useEditor({
        extensions: createExtensions(() => null, { views: true }),
        content: toProseMirror(d) as never,
      });
      return <EditorContent editor={editor} />;
    }
    render(<Host />);
    expect(await screen.findByText('report.pdf')).toBeTruthy();
    expect(screen.getByText(/2\.0 KB/)).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Save as…' }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });
});

describe("applying another device's changes to an open editor", () => {
  it('replaces the content without an undo step or an update event, keeping the cursor nearby', () => {
    const e = make(doc(p('one', 'a'), p('two', 'b')));
    const onUpdate = vi.fn();
    e.on('update', onUpdate);
    e.commands.setTextSelection(6);
    replaceContent(e, doc(p('one', 'a'), p('two (remote)', 'b'), p('three', 'c')));
    expect(fromProseMirror(e.getJSON() as never).blocks.map((b) => b.text)).toEqual([
      'one',
      'two (remote)',
      'three',
    ]);
    expect(onUpdate).not.toHaveBeenCalled();
    expect(e.state.selection.from).toBe(6);
    e.commands.undo();
    expect(fromProseMirror(e.getJSON() as never).blocks).toHaveLength(3);
  });

  it('compares documents structurally', () => {
    expect(sameDocument(doc(p('a', '1')), doc(p('a', '1')))).toBe(true);
    expect(sameDocument(doc(p('a', '1')), doc(p('b', '1')))).toBe(false);
    expect(sameDocument(doc(p('a', '1')), doc(p('a', '2')))).toBe(false);
  });
});
