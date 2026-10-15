import { beforeEach, describe, expect, it, vi } from 'vitest';

async function setup() {
  localStorage.clear();
  vi.resetModules();
  const { actions, getState, childrenOf } = await import('./app');
  await actions.init();
  const titles = (parent: string | null) =>
    childrenOf(getState().pages, parent).map((p) => p.title);
  const id = (title: string) => getState().pages.find((p) => p.title === title)!.id;
  return { actions, getState, titles, id };
}

describe('moving pages (drag-and-drop and keyboard backends)', () => {
  let t: Awaited<ReturnType<typeof setup>>;
  beforeEach(async () => {
    t = await setup();
    for (const name of ['A', 'B', 'C']) {
      const page = await t.actions.createPage(null);
      await t.actions.rename(page!.id, name);
    }
  });

  it('places before, after and inside another page', async () => {
    const root = ['Welcome to Notercrux', 'Reading list'];
    expect(t.titles(null)).toEqual([...root, 'A', 'B', 'C']);
    await t.actions.place(t.id('C'), t.id('A'), 'before');
    expect(t.titles(null)).toEqual([...root, 'C', 'A', 'B']);
    await t.actions.place(t.id('C'), t.id('B'), 'after');
    expect(t.titles(null)).toEqual([...root, 'A', 'B', 'C']);
    await t.actions.place(t.id('A'), t.id('B'), 'inside');
    expect(t.titles(t.id('B'))).toEqual(['A']);
    expect(t.titles(null)).toEqual([...root, 'B', 'C']);
  });

  it('shift reorders, indents under the previous sibling and outdents after the parent', async () => {
    await t.actions.shift(t.id('B'), 'up');
    expect(t.titles(null).slice(-3)).toEqual(['B', 'A', 'C']);
    await t.actions.shift(t.id('C'), 'indent');
    expect(t.titles(t.id('A'))).toEqual(['C']);
    await t.actions.shift(t.id('C'), 'outdent');
    expect(t.titles(null).slice(-3)).toEqual(['B', 'A', 'C']);
    await t.actions.shift(t.id('B'), 'indent'); // B's previous sibling is "Reading list"
    expect(t.titles(t.id('Reading list'))).toEqual(['B']);
  });

  it('does not allow dropping a page into its own subtree', async () => {
    await t.actions.place(t.id('B'), t.id('A'), 'inside');
    await t.actions.place(t.id('A'), t.id('B'), 'inside'); // A is B's parent now
    expect(t.titles(t.id('A'))).toEqual(['B']);
    expect(t.getState().error).toMatch(/descendant/);
  });
});
