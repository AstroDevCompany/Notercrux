import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { Dialog } from './Dialog';

beforeEach(() => {
  localStorage.clear();
  // jsdom lacks these.
  Element.prototype.scrollIntoView = vi.fn();
  window.matchMedia = ((q: string) => ({
    matches: false,
    media: q,
    addEventListener: () => {},
    removeEventListener: () => {},
  })) as unknown as typeof window.matchMedia;
});
afterEach(cleanup);

describe('Dialog', () => {
  it('moves focus in, traps Tab, closes on Escape and restores focus', () => {
    const opener = document.createElement('button');
    document.body.append(opener);
    opener.focus();
    const onClose = vi.fn();
    const { unmount } = render(
      <Dialog title="Hello" onClose={onClose}>
        <button>First</button>
        <button>Last</button>
      </Dialog>,
    );
    const dialog = screen.getByRole('dialog', { name: 'Hello' });
    expect(dialog.contains(document.activeElement)).toBe(true);
    screen.getByText('Last').focus();
    fireEvent.keyDown(screen.getByText('Last'), { key: 'Tab' });
    expect(document.activeElement).toBe(screen.getByText('First'));
    fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledOnce();
    unmount();
    expect(document.activeElement).toBe(opener);
  });
});

describe('Command palette + page tree', () => {
  it('lists pages in an accessible tree, filters in the palette, and opens the chosen page', async () => {
    vi.resetModules();
    const { default: App } = await import('../App');
    const { actions, getState } = await import('../state/app');
    render(<App />);

    const tree = await screen.findByRole('tree', { name: 'Pages' });
    const items = tree.querySelectorAll('[role=treeitem]');
    expect(items.length).toBeGreaterThanOrEqual(2);
    expect(tree.querySelector('[aria-expanded]')).not.toBeNull();
    expect(tree.querySelectorAll('[tabindex="0"]')).toHaveLength(1); // roving tab stop

    actions.setPalette(true);
    const box = await screen.findByRole('combobox', { name: /search pages/i });
    fireEvent.change(box, { target: { value: 'reading' } });
    const option = await screen.findByRole('option', { name: /Reading list/ });
    expect(option.getAttribute('aria-selected')).toBe('true');
    fireEvent.keyDown(box, { key: 'Enter' });

    await waitFor(() => {
      const current = getState().pages.find((p) => p.id === getState().currentId);
      expect(current?.title).toBe('Reading list');
    });
    expect(screen.queryByRole('dialog', { name: 'Command palette' })).toBeNull();
  });

  it('keyboard navigation moves focus between rows and expands with ArrowRight', async () => {
    vi.resetModules();
    const { default: App } = await import('../App');
    render(<App />);
    const tree = await screen.findByRole('tree', { name: 'Pages' });
    const first = tree.querySelector<HTMLElement>('[role=treeitem]')!;
    first.focus();
    const before = tree.querySelectorAll('[role=treeitem]').length;
    fireEvent.keyDown(first, { key: 'ArrowRight' }); // first row has a nested page
    await waitFor(() =>
      expect(tree.querySelectorAll('[role=treeitem]').length).toBeGreaterThan(before),
    );
  });
});
