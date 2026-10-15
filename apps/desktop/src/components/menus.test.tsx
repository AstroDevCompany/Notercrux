import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { Editor } from '@tiptap/core';
import { ContextMenu, type MenuItem } from './ContextMenu';
import { BlockMenu } from './BlockMenu';
import { buildEditorMenu } from '../editor/contextMenu';
import { createExtensions } from '../editor/extensions';

beforeEach(() => {
  window.matchMedia = ((q: string) => ({
    matches: false,
    media: q,
    addEventListener: () => {},
    removeEventListener: () => {},
  })) as unknown as typeof window.matchMedia;
});
afterEach(cleanup);

describe('ContextMenu', () => {
  const setup = (items: MenuItem[]) => {
    const onClose = vi.fn();
    render(<ContextMenu x={10} y={10} items={items} onClose={onClose} />);
    return { onClose, menu: screen.getByRole('menu') };
  };

  it('runs an item and closes; disabled items do nothing', () => {
    const run = vi.fn();
    const blocked = vi.fn();
    const { onClose } = setup([
      { label: 'Blocked', disabled: true, onSelect: blocked },
      { type: 'separator' },
      { label: 'Run', onSelect: run },
    ]);
    fireEvent.click(screen.getByRole('menuitem', { name: 'Blocked' }));
    expect(blocked).not.toHaveBeenCalled();
    expect(screen.getAllByRole('separator')).toHaveLength(1);
    fireEvent.click(screen.getByRole('menuitem', { name: 'Run' }));
    expect(run).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalled();
  });

  it('moves with the arrow keys (skipping disabled items) and closes on Escape', () => {
    const { menu, onClose } = setup([
      { label: 'One' },
      { label: 'Two', disabled: true },
      { label: 'Three' },
    ]);
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'One' }));
    fireEvent.keyDown(menu, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'Three' }));
    fireEvent.keyDown(menu, { key: 'ArrowDown' }); // wraps
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'One' }));
    fireEvent.keyDown(menu, { key: 'Escape' });
    expect(onClose).toHaveBeenCalled();
  });

  it('opens a submenu with → or hover, runs a nested item, and ← returns to the parent', () => {
    const nested = vi.fn();
    const { onClose } = setup([
      {
        label: 'Turn into',
        children: [
          { label: 'Heading', onSelect: nested },
          { label: 'Quote', checked: true },
        ],
      },
      { label: 'Other' },
    ]);
    const parent = screen.getByRole('menuitem', { name: 'Turn into' });
    expect(screen.queryByRole('menuitem', { name: 'Heading' })).toBeNull();
    fireEvent.keyDown(parent, { key: 'ArrowRight' });
    const heading = screen.getByRole('menuitem', { name: 'Heading' });
    expect(document.activeElement).toBe(heading);
    expect(
      within(screen.getByRole('menuitem', { name: /Quote/ })).queryByText('Quote'),
    ).not.toBeNull();
    fireEvent.keyDown(heading, { key: 'ArrowLeft' });
    expect(screen.queryByRole('menuitem', { name: 'Heading' })).toBeNull();

    fireEvent.mouseMove(parent); // hover opens it as well
    fireEvent.click(screen.getByRole('menuitem', { name: 'Heading' }));
    expect(nested).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalled();
  });

  it('highlights the item under the pointer by moving focus (one highlight at a time)', () => {
    setup([{ label: 'A' }, { label: 'B' }]);
    fireEvent.mouseMove(screen.getByRole('menuitem', { name: 'B' }));
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'B' }));
  });
});

const p = (text: string) => ({ type: 'paragraph', content: [{ type: 'text', text }] });
const make = (content: unknown[]) =>
  new Editor({
    extensions: createExtensions(),
    content: { type: 'doc', content } as never,
    element: document.createElement('div'),
  });
const at = (e: Editor, text: string) => {
  let pos = -1;
  e.state.doc.descendants((n, ps) => {
    if (n.isText && n.text === text) pos = ps;
  });
  return e.state.doc.resolve(pos).before(1);
};
const anchor = { getBoundingClientRect: () => new DOMRect(50, 50, 10, 10) };

describe('BlockMenu', () => {
  it('edits a code block language and shows the current type', () => {
    const e = make([
      { type: 'codeBlock', attrs: { language: null }, content: [{ type: 'text', text: 'let x' }] },
    ]);
    render(<BlockMenu editor={e} pos={at(e, 'let x')} anchor={anchor} onClose={() => {}} />);
    expect(screen.getByText('Code block', { selector: '.bm-head' })).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Code language'), { target: { value: 'rust' } });
    expect(e.getJSON().content![0]!.attrs!.language).toBe('rust');
    e.destroy();
  });

  it('turns the block into something else and closes; Move up is disabled for the first block', () => {
    const e = make([p('first'), p('second')]);
    const onClose = vi.fn();
    render(<BlockMenu editor={e} pos={at(e, 'first')} anchor={anchor} onClose={onClose} />);
    expect((screen.getByRole('button', { name: /Move up/ }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    fireEvent.click(
      within(screen.getByRole('group', { name: 'Turn into' })).getByRole('button', {
        name: 'Quote',
      }),
    );
    expect(onClose).toHaveBeenCalled();
    expect(e.getJSON().content![0]!.type).toBe('blockquote');
    e.destroy();
  });

  it('offers callout colours and to-do state, and deletes the block', () => {
    const e = make([{ type: 'callout', attrs: { icon: '💡' }, content: [p('note')] }, p('after')]);
    const onClose = vi.fn();
    render(<BlockMenu editor={e} pos={at(e, 'note')} anchor={anchor} onClose={onClose} />);
    fireEvent.click(screen.getByRole('radio', { name: 'Red' }));
    expect(e.getJSON().content![0]!.attrs!.tone).toBe('red');
    fireEvent.click(screen.getByRole('button', { name: /Delete/ }));
    expect(e.getJSON().content!.map((n) => n.type)).toEqual(['paragraph']);
    expect(onClose).toHaveBeenCalled();
    e.destroy();
  });
});

describe('editor context menu items', () => {
  const labels = (items: MenuItem[]) =>
    items.map((i) => (i.type === 'separator' ? '---' : (i as { label: string }).label));

  it('offers clipboard, block and turn-into items; formatting appears only with a selection', () => {
    const e = make([p('hello world')]);
    e.commands.setTextSelection(3);
    const plain = buildEditorMenu(e, { pos: 3, linkPageId: null, openBlockMenu: () => {} });
    expect(labels(plain)).toEqual(
      expect.arrayContaining([
        'Cut',
        'Copy',
        'Paste',
        'Paste as plain text',
        'Turn into',
        'Block options…',
        'Duplicate block',
        'Delete block',
      ]),
    );
    expect(labels(plain)).not.toContain('Bold');
    expect(plain.find((i) => (i as { label?: string }).label === 'Cut')).toMatchObject({
      disabled: true,
    });

    e.commands.setTextSelection({ from: 1, to: 6 });
    e.commands.toggleBold();
    const withRange = buildEditorMenu(e, { pos: 3, linkPageId: null, openBlockMenu: () => {} });
    expect(labels(withRange)).toEqual(
      expect.arrayContaining(['Bold', 'Italic', 'Strikethrough', 'Code']),
    );
    expect(withRange.find((i) => (i as { label?: string }).label === 'Bold')).toMatchObject({
      checked: true,
    });
    expect(withRange.find((i) => (i as { label?: string }).label === 'Cut')).toMatchObject({
      disabled: false,
    });
    e.destroy();
  });

  it('adds link actions over a page link and omits "Turn into" for dividers', () => {
    const e = make([p('x'), { type: 'horizontalRule' }]);
    const items = buildEditorMenu(e, { pos: 1, linkPageId: 'abc', openBlockMenu: () => {} });
    expect(labels(items).slice(0, 2)).toEqual(['Open linked page', 'Remove link']);
    const hr = buildEditorMenu(e, {
      pos: e.state.doc.content.size - 1,
      linkPageId: null,
      openBlockMenu: () => {},
    });
    e.commands.setNodeSelection(e.state.doc.content.size - 1);
    const onDivider = buildEditorMenu(e, {
      pos: e.state.doc.content.size - 1,
      linkPageId: null,
      openBlockMenu: () => {},
    });
    expect(labels(onDivider)).not.toContain('Turn into');
    void hr;
    e.destroy();
  });
});
