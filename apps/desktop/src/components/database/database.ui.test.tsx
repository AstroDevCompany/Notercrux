import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';

beforeEach(() => {
  localStorage.clear();
  window.matchMedia = ((q: string) => ({
    matches: false,
    media: q,
    addEventListener: () => {},
    removeEventListener: () => {},
  })) as unknown as typeof window.matchMedia;
  window.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
  Element.prototype.scrollIntoView = vi.fn();
});
afterEach(cleanup);

async function setup() {
  vi.resetModules();
  const { api } = await import('../../api');
  const { actions, getState } = await import('../../state/app');
  await actions.init();
  const db = await api.createDatabase(null, 'Tasks');
  const schema = await api.getSchema(db.id);
  schema.properties = [
    {
      id: 'status',
      name: 'Status',
      type: 'select',
      options: [
        { id: 'todo', name: 'To do' },
        { id: 'done', name: 'Done' },
      ],
    },
    { id: 'points', name: 'Points', type: 'number' },
  ];
  await api.setSchema(db.id, schema);
  for (const [title, status, points] of [
    ['Write docs', 'todo', 3],
    ['Ship v1', 'done', 8],
    ['Fix bug', 'todo', 1],
  ] as const) {
    const r = await api.createPage(db.id, title);
    await api.setProperty(r.id, 'status', status);
    await api.setProperty(r.id, 'points', points);
  }
  await actions.refresh();
  const { DatabaseView } = await import('./DatabaseView');
  const page = getState().pages.find((p) => p.id === db.id)!;
  return { api, db, schema, page, DatabaseView, actions };
}

const titles = () =>
  screen
    .queryAllByRole('row')
    .slice(1)
    .map((r) => (within(r).getByLabelText('Name') as HTMLInputElement).value);

describe('DatabaseView', () => {
  it('shows records, applies a saved filter and sort, and reports counts', async () => {
    const { api, db, schema, page, DatabaseView, actions } = await setup();
    render(<DatabaseView page={page} />);
    await waitFor(() => expect(titles()).toEqual(['Write docs', 'Ship v1', 'Fix bug']));
    expect(screen.getByRole('status').textContent).toBe('3 records');

    schema.views[0]!.filters = [{ property: 'status', op: 'equals', value: 'todo' }];
    schema.views[0]!.sorts = [{ property: 'points', direction: 'asc' }];
    await api.setSchema(db.id, schema);
    await act(async () => void (await actions.refresh()));
    await waitFor(() => expect(titles()).toEqual(['Fix bug', 'Write docs']));
    expect(screen.getByRole('status').textContent).toBe('2 of 3 records');
  });

  it('edits cell values and persists them', async () => {
    const { api, db, page, DatabaseView } = await setup();
    render(<DatabaseView page={page} />);
    await waitFor(() => expect(titles()).toHaveLength(3));
    const row = screen.getAllByRole('row')[1]!;
    const points = within(row).getByLabelText('Points') as HTMLInputElement;
    fireEvent.focus(points);
    fireEvent.change(points, { target: { value: '13' } });
    fireEvent.blur(points);
    await waitFor(async () => expect((await api.listRecords(db.id))[0]!.values.points).toBe(13));

    // Garbage in a number cell is rejected, not saved.
    fireEvent.focus(points);
    fireEvent.change(points, { target: { value: 'abc' } });
    fireEvent.blur(points);
    expect(points.value).toBe('13');
    expect((await api.listRecords(db.id))[0]!.values.points).toBe(13);
  });

  it('adds a property and a view through the controls', async () => {
    const { api, db, page, DatabaseView } = await setup();
    render(<DatabaseView page={page} />);
    await waitFor(() => expect(titles()).toHaveLength(3));

    fireEvent.click(screen.getByRole('button', { name: 'Properties' }));
    fireEvent.change(await screen.findByLabelText('New property name'), {
      target: { value: 'Due' },
    });
    fireEvent.change(screen.getByLabelText('New property type'), { target: { value: 'date' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add property' }));
    await waitFor(async () =>
      expect((await api.getSchema(db.id)).properties.map((p) => p.name)).toEqual([
        'Status',
        'Points',
        'Due',
      ]),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Add view' }));
    await waitFor(async () => expect((await api.getSchema(db.id)).views).toHaveLength(2));
  });
});
