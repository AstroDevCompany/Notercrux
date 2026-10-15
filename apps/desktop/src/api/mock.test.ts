import { beforeEach, describe, expect, it, vi } from 'vitest';

async function fresh() {
  localStorage.clear();
  vi.resetModules();
  return (await import('./mock')).mockApi;
}

describe('mock api (browser preview backend)', () => {
  let api: Awaited<ReturnType<typeof fresh>>;
  beforeEach(async () => {
    api = await fresh();
  });

  it('refuses to move a page into its own descendant and leaves the tree unchanged', async () => {
    const before = await api.listPages();
    const root = before.find(
      (p) => p.parentId === null && before.some((c) => c.parentId === p.id),
    )!;
    const child = before.find((p) => p.parentId === root.id)!;
    await expect(api.movePage(root.id, child.id, 0)).rejects.toThrow(/descendant/);
    expect(await api.listPages()).toEqual(before);
  });

  it('archives a subtree and restores a child under an archived parent at the root', async () => {
    const pages = await api.listPages();
    const root = pages.find((p) => p.parentId === null && pages.some((c) => c.parentId === p.id))!;
    const child = pages.find((p) => p.parentId === root.id)!;
    await api.archivePage(root.id);
    expect(
      (await api.listPages()).every((p) =>
        p.id === root.id || p.id === child.id ? p.archivedAt : true,
      ),
    ).toBe(true);
    await api.restorePage(child.id);
    const restored = (await api.listPages()).find((p) => p.id === child.id)!;
    expect(restored.archivedAt).toBeNull();
    expect(restored.parentId).toBeNull();
  });

  it('finds pages by title and body text, but not archived ones', async () => {
    const page = await api.createPage(null, 'Quarterly plan');
    await api.saveDocument(page.id, {
      version: 1,
      blocks: [{ id: 'b1', type: 'paragraph', text: 'launch the zeppelin' }],
    });
    expect((await api.searchPages('quart'))[0]?.page.id).toBe(page.id);
    expect((await api.searchPages('zeppelin'))[0]?.page.id).toBe(page.id);
    await api.archivePage(page.id);
    expect(await api.searchPages('zeppelin')).toEqual([]);
  });
});
