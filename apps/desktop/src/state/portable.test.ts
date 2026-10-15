import { beforeEach, describe, expect, it, vi } from 'vitest';

const files = vi.hoisted(() => ({
  pickTextFile: vi.fn(),
  saveTextFile: vi.fn(async () => true),
}));
vi.mock('../lib/files', () => files);

async function setup() {
  localStorage.clear();
  vi.resetModules();
  const app = await import('./app');
  const { api } = await import('../api');
  await app.actions.init();
  return { ...app, api };
}

describe('import and export', () => {
  beforeEach(() => {
    files.pickTextFile.mockReset();
    files.saveTextFile.mockClear();
  });

  it('imports Markdown as a new page, using the H1 as title and reporting unsupported content', async () => {
    const { actions, getState, api } = await setup();
    files.pickTextFile.mockResolvedValue({
      name: 'notes.md',
      contents: '# Trip plan\n\n- pack\n- [x] book\n\n| a | b |\n',
    });
    await actions.importFile();
    const page = getState().pages.find((p) => p.title === 'Trip plan')!;
    expect(page).toBeTruthy();
    expect(getState().currentId).toBe(page.id);
    const doc = await api.loadDocument(page.id);
    expect(doc.blocks.map((b) => b.type)).toEqual(['bulletedListItem', 'todo', 'paragraph']);
    expect(getState().notice?.text).toMatch(/with 1 note/);
    expect(getState().notice?.details[0]).toMatch(/Tables/);
  });

  it('falls back to the file name for the title, and reports invalid JSON instead of importing it', async () => {
    const { actions, getState } = await setup();
    files.pickTextFile.mockResolvedValue({ name: 'groceries.txt', contents: 'milk' });
    await actions.importFile();
    expect(getState().pages.some((p) => p.title === 'groceries')).toBe(true);

    const before = getState().pages.length;
    files.pickTextFile.mockResolvedValue({ name: 'x.json', contents: '{"format":"other"}' });
    await actions.importFile();
    expect(getState().pages).toHaveLength(before);
    expect(getState().error).toMatch(/not a Notercrux page export/);
  });

  it('exports JSON that can be imported back losslessly', async () => {
    const { actions, getState, api } = await setup();
    const page = getState().pages.find((p) => p.title === 'Welcome to Notercrux')!;
    const original = await api.loadDocument(page.id);
    await actions.exportPage(page.id, 'json');
    const [name, ext, text] = files.saveTextFile.mock.calls[0] as unknown as [
      string,
      string,
      string,
    ];
    expect([name, ext]).toEqual(['Welcome to Notercrux', 'json']);

    files.pickTextFile.mockResolvedValue({ name: 'Welcome.json', contents: text });
    await actions.importFile();
    const copy = getState().pages.find((p) => p.id === getState().currentId)!;
    expect(copy.id).not.toBe(page.id);
    expect(await api.loadDocument(copy.id)).toEqual(original);
  });
});
